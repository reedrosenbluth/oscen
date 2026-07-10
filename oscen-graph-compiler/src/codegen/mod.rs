use crate::ast::{BinaryOp, EndpointKind, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::ir::graph::{EdgeId, EdgeKernel, IrEdge, IrGraph, IrNode, IrNodeKind, NodeId};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;
use syn::{Expr, Result};

pub(crate) mod helpers;
use helpers::*;

mod emit_edge;
mod emit_frame;
mod emit_node;
mod emit_params;
mod emit_struct;
mod validate_names;

/// The frame type shared by all of a graph's top-level stream endpoints, used
/// to type the `BlockRender<F>` impl and the stream block buffers.
// Transient value computed and matched immediately; never stored in bulk, so
// the size of the `Frame` variant does not matter.
#[allow(clippy::large_enum_variant)]
enum BlockFrameTy {
    /// All stream endpoints are mono `f32`.
    Mono,
    /// All stream endpoints are the same `Frame<N>` type.
    Frame(syn::Type),
    /// Stream endpoints mix `f32` with `Frame<N>` (or differing `Frame<N>`):
    /// offline rendering is out of scope (the impl emits a `compile_error!`).
    Mixed,
}

pub fn generate(
    ir: &IrGraph,
    source_tokens: &TokenStream,
) -> std::result::Result<TokenStream, Diagnostics> {
    // lower() has already run analysis + validation. Codegen consumes the
    // IR directly. Static graphs require a name (already enforced by
    // lower()), so we just emit. `source_tokens` is the original `graph!`
    // body, hashed into the endpoint-manifest export name.
    let ctx = CodegenContext::new(ir, source_tokens);
    ctx.check_generated_name_collisions()
        .map_err(Diagnostics::from)?;
    ctx.generate_static_struct().map_err(Diagnostics::from)
}

/// Codegen context: thin wrapper around `&IrGraph` plus precomputed lookup
/// tables. The IR is the single source of truth; this struct only caches a
/// name → `NodeId` map and per-edge-index → `EdgeId` mapping for hot loops.
struct CodegenContext<'a> {
    ir: &'a IrGraph,
    /// Node name → `NodeId` map. The same name uniqueness invariant that
    /// `lower::collect_declarations` enforces means this is well-defined.
    name_to_id: HashMap<String, NodeId>,
    /// The original `graph!` body tokens, used only to disambiguate the
    /// endpoint-manifest `#[macro_export]` name (see
    /// [`crate::manifest::emit_manifest_export`]).
    source_tokens: &'a TokenStream,
}

impl<'a> CodegenContext<'a> {
    fn new(ir: &'a IrGraph, source_tokens: &'a TokenStream) -> Self {
        let mut name_to_id = HashMap::new();
        for (id, node) in &ir.nodes {
            name_to_id.insert(node.name.to_string(), id);
        }
        Self {
            ir,
            name_to_id,
            source_tokens,
        }
    }

    // ---------- IR lookup helpers ----------

    fn name(&self) -> &syn::Ident {
        &self.ir.name
    }

    fn nih_params(&self) -> bool {
        self.ir.nih_params
    }

    fn find_node_by_name(&self, name: &str) -> Option<&IrNode> {
        self.name_to_id.get(name).map(|&id| &self.ir.nodes[id])
    }

    fn find_node_by_ident(&self, ident: &syn::Ident) -> Option<&IrNode> {
        self.find_node_by_name(&ident.to_string())
    }

    /// Iterate inputs in source order.
    fn inputs(&self) -> impl Iterator<Item = &IrNode> {
        self.ir.inputs.iter().map(|&id| &self.ir.nodes[id])
    }

    /// Iterate outputs in source order.
    fn outputs(&self) -> impl Iterator<Item = &IrNode> {
        self.ir.outputs.iter().map(|&id| &self.ir.nodes[id])
    }

    /// Iterate processor (non-IO) nodes in topological order.
    fn nodes(&self) -> impl Iterator<Item = &IrNode> {
        self.ir.processors.iter().map(|&id| &self.ir.nodes[id])
    }

    /// Iterate edges in canonical source order, yielding (edge_index, edge).
    /// `edge_index` is used by codegen to name per-edge resampler fields and
    /// buffers — it must agree with `IrGraph::edge_order` ordering.
    fn edges(&self) -> impl Iterator<Item = (usize, &IrEdge)> {
        self.ir
            .edge_order
            .iter()
            .enumerate()
            .map(|(i, &eid)| (i, &self.ir.edges[eid]))
    }

    /// True if any cross-rate node uses an oversampling factor > 1.
    fn max_factor(&self) -> u32 {
        let mut max = 1u32;
        for node in self.nodes() {
            if let NodeRate::Up(f) = node.rate {
                max = lcm(max, f);
            }
        }
        max
    }

    fn input_kind(&self, name: &syn::Ident) -> Option<EndpointKind> {
        let node = self.find_node_by_ident(name)?;
        if !matches!(node.kind, IrNodeKind::Input { .. }) {
            return None;
        }
        node.endpoints.get(name).map(|e| e.kind)
    }

    fn output_kind(&self, name: &syn::Ident) -> Option<EndpointKind> {
        let node = self.find_node_by_ident(name)?;
        if !matches!(node.kind, IrNodeKind::Output) {
            return None;
        }
        node.endpoints.get(name).map(|e| e.kind)
    }

    /// The declared frame type of a top-level stream endpoint `name`, recognized
    /// as a non-`f32` `AudioFrame` (`Frame<...>`/`Stereo`/`Mono`/`Quad`).
    /// Returns `None` for an absent, `f32`, or unrecognized annotation — all of
    /// which mean mono `f32` (this keeps legacy `[f32; N]` stream annotations
    /// no-ops, as before).
    fn endpoint_frame_ty(&self, name: &syn::Ident) -> Option<syn::Type> {
        let node = self.find_node_by_ident(name)?;
        let ty = node.endpoints.get(name)?.ty.as_ref()?;
        if let syn::Type::Path(tp) = ty {
            if let Some(seg) = tp.path.segments.last() {
                return match seg.ident.to_string().as_str() {
                    "Frame" | "Stereo" | "Mono" | "Quad" => Some(ty.clone()),
                    _ => None,
                };
            }
        }
        None
    }

    /// The Rust type to emit for a top-level stream endpoint field/buffer: its
    /// recognized frame type, or `f32` (mono default).
    fn stream_field_ty(&self, name: &syn::Ident) -> TokenStream {
        match self.endpoint_frame_ty(name) {
            Some(ty) => quote! { #ty },
            None => quote! { f32 },
        }
    }

    /// Initializer expressions `(scalar, block_buffer)` for a top-level stream
    /// endpoint. Mono `f32` keeps the literal `0.0f32` form (byte-identical to
    /// the pre-generalization codegen); frame types use `F::default()`.
    fn stream_init_exprs(&self, name: &syn::Ident) -> (TokenStream, TokenStream) {
        match self.endpoint_frame_ty(name) {
            None => (
                quote! { 0.0f32 },
                quote! { [0.0f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] },
            ),
            Some(ty) => (
                quote! { <#ty as ::core::default::Default>::default() },
                quote! {
                    [<#ty as ::core::default::Default>::default();
                        ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE]
                },
            ),
        }
    }

    /// The single frame type shared by every top-level stream endpoint, for the
    /// `BlockRender<F>` impl. See [`BlockFrameTy`].
    fn block_render_frame_ty(&self) -> BlockFrameTy {
        let mut frame: Option<syn::Type> = None;
        let mut saw_mono = false;
        let stream_endpoints = self
            .inputs()
            .filter(|n| self.input_kind(&n.name) == Some(EndpointKind::Stream))
            .chain(
                self.outputs()
                    .filter(|n| self.output_kind(&n.name) == Some(EndpointKind::Stream)),
            );
        for node in stream_endpoints {
            match self.endpoint_frame_ty(&node.name) {
                None => saw_mono = true,
                // Compare (and emit) the canonical `::oscen::frame::…`
                // spelling: a wildcard-hoisted endpoint carries its frame
                // type fully qualified via the child's manifest, and must
                // compare equal to a locally declared bare `Frame<2>`.
                Some(t) => {
                    let t = qualified_frame_ty(&t);
                    match &frame {
                        Some(existing)
                            if quote!(#existing).to_string() != quote!(#t).to_string() =>
                        {
                            return BlockFrameTy::Mixed;
                        }
                        _ => frame = Some(t),
                    }
                }
            }
        }
        match frame {
            None => BlockFrameTy::Mono,
            Some(_) if saw_mono => BlockFrameTy::Mixed,
            Some(t) => BlockFrameTy::Frame(t),
        }
    }

    fn is_input(&self, name: &syn::Ident) -> bool {
        matches!(
            self.find_node_by_ident(name).map(|n| &n.kind),
            Some(IrNodeKind::Input { .. })
        )
    }

    fn is_output(&self, name: &syn::Ident) -> bool {
        matches!(
            self.find_node_by_ident(name).map(|n| &n.kind),
            Some(IrNodeKind::Output)
        )
    }

    /// Look up the rate annotation for a node by name. Falls back to `Same`
    /// for unknown names (defensive — should never happen for nodes that
    /// passed type checking).
    fn node_rate(&self, name: &syn::Ident) -> NodeRate {
        self.find_node_by_ident(name)
            .map(|n| n.rate)
            .unwrap_or(NodeRate::Same)
    }

    /// Get the array size for a node, if it is a NodeArray.
    fn get_node_array_size(&self, name: &syn::Ident) -> Option<usize> {
        let node = self.find_node_by_ident(name)?;
        match &node.kind {
            IrNodeKind::NodeArray { len, .. } => Some(*len),
            _ => None,
        }
    }

    /// Get the constructor `syn::Expr` for a processor/array node.
    fn node_ctor_expr<'b>(&self, node: &'b IrNode) -> Option<&'b Expr> {
        match &node.kind {
            IrNodeKind::Processor { ctor_expr, .. } | IrNodeKind::NodeArray { ctor_expr, .. } => {
                Some(ctor_expr)
            }
            _ => None,
        }
    }

    /// Constructor call tokens for a single element of a processor/array
    /// node: a bare path constructor (`Type`) becomes `Type::new()`, any
    /// other expression is used as written. Shared by `new()`'s node init
    /// and the param-descriptor probe so a probe can never drift from the
    /// value `new()` actually constructs.
    fn node_ctor_tokens(&self, node: &IrNode) -> Option<TokenStream> {
        let ctor_expr = self.node_ctor_expr(node)?;
        Some(match ctor_expr {
            Expr::Path(path) => quote! { #path::new() },
            _ => quote! { #ctor_expr },
        })
    }

    /// Get the node type path for a processor/array node.
    fn node_type_path<'b>(&self, node: &'b IrNode) -> Option<&'b syn::Path> {
        match &node.kind {
            IrNodeKind::Processor { ty, .. } | IrNodeKind::NodeArray { ty, .. } => ty.as_ref(),
            _ => None,
        }
    }

    /// Default expression for an input node.
    fn input_default<'b>(&self, node: &'b IrNode) -> Option<&'b Expr> {
        match &node.kind {
            IrNodeKind::Input { default, .. } => default.as_ref(),
            _ => None,
        }
    }

    /// `ParamSpec` for an input node.
    fn input_spec<'b>(&self, node: &'b IrNode) -> Option<&'b crate::ast::ParamSpec> {
        match &node.kind {
            IrNodeKind::Input { spec, .. } => spec.as_ref(),
            _ => None,
        }
    }

    /// Hoist source (`input <node>.<endpoint>;`) for an input node.
    fn input_hoist<'b>(&self, node: &'b IrNode) -> Option<&'b crate::ast::HoistSource> {
        match &node.kind {
            IrNodeKind::Input { hoist, .. } => hoist.as_ref(),
            _ => None,
        }
    }

    /// The declared payload type of a TYPED graph value endpoint (input or
    /// output) by name. `None` for plain f32 params and everything else.
    /// Thin facade over [`crate::ir::graph::EndpointInfo::typed_value_ty`],
    /// the single definition of the TYPED classification.
    fn typed_value_ty(&self, name: &syn::Ident) -> Option<&syn::Type> {
        let node = self.find_node_by_ident(name)?;
        self.ir.typed_value_endpoint_ty(node.id, name)
    }

    /// Value inputs that are f32 *parameters* — the partition that drives the
    /// `{Graph}Param` enum, descriptor table, `set_param`/`get_param`
    /// dispatch, and the nih-plug params struct. TYPED value inputs are
    /// excluded everywhere; they only get their typed setter. Both
    /// `generate_param_registry` and `generate_nih_params_struct` must build
    /// from this list so positional descriptor indices stay aligned.
    fn param_value_inputs(&self) -> Vec<&IrNode> {
        self.inputs()
            .filter(|n| {
                matches!(self.input_kind(&n.name), Some(EndpointKind::Value))
                    && self.typed_value_ty(&n.name).is_none()
            })
            .collect()
    }

    /// True when an edge moves a TYPED value payload (either side is a typed
    /// graph value endpoint). Such edges bypass the f32 cross-rate kernel
    /// machinery: they are latched (copied at the outer-block boundary).
    fn edge_is_typed_value(&self, edge: &IrEdge) -> bool {
        self.ir.edge_is_typed_value(edge)
    }

    /// Check if an input has a ramp annotation and return the default ramp frames.
    fn is_ramped_input(&self, name: &syn::Ident) -> Option<usize> {
        let node = self.find_node_by_ident(name)?;
        if !matches!(node.kind, IrNodeKind::Input { .. }) {
            return None;
        }
        if !matches!(
            node.endpoints.get(name).map(|e| e.kind),
            Some(EndpointKind::Value)
        ) {
            return None;
        }
        self.input_spec(node).and_then(|s| s.ramp)
    }

    /// Project the EndpointAt marker token for an IR endpoint.
    /// Returns `Some((<NodeTypePath>, <NodeTypeName__field__Ep marker path>))`
    /// when the endpoint's node has a recorded `node_type` whose path is
    /// multi-segment. Returns `None` otherwise.
    fn endpoint_marker_tokens(
        &self,
        ep: &crate::ir::expr::IrEndpoint,
    ) -> Option<(TokenStream, TokenStream)> {
        let node = &self.ir.nodes[ep.node];
        let path = self.node_type_path(node)?;
        let assoc_ident = syn::Ident::new(
            &format!("{}__Ep", ep.endpoint),
            proc_macro2::Span::call_site(),
        );
        Some((quote! { #path }, quote! { <#path>::#assoc_ident }))
    }

    /// Extract the `IrEndpoint` from an `IrExpr`, if the expression is a
    /// plain `Endpoint` variant. Returns `None` for compound expressions
    /// (Binary, MethodCall, Call, Literal).
    fn ir_expr_as_endpoint(expr: &crate::ir::expr::IrExpr) -> Option<&crate::ir::expr::IrEndpoint> {
        if let crate::ir::expr::IrExprKind::Endpoint(ep) = &expr.kind {
            Some(ep)
        } else {
            None
        }
    }

    /// Field-access tokens to reach an edge's resampler kernel: `.kernel`
    /// when the resampler field is a projected `CrossRateKernel` state
    /// wrapper, empty when it is the concrete kernel type directly.
    fn edge_kernel_access(&self, edge: &IrEdge) -> TokenStream {
        if self.cross_rate_kernel_state_type(edge).is_some() {
            quote! { .kernel }
        } else {
            quote! {}
        }
    }

    /// Emit the `<() as CrossRateKernel<SrcKind, DstKind, Policy, N, Dir>>::State`
    /// projection for an edge. Returns `None` if either endpoint can't be
    /// projected (e.g., compound source like `osc.output * 2.0`, or a graph
    /// input/output that doesn't have a derive-emitted EndpointAt marker), in
    /// which case callers fall back to `kernel_up_type` / `kernel_down_type`.
    fn cross_rate_kernel_state_type(&self, edge: &IrEdge) -> Option<TokenStream> {
        // Kind-gate: only project for stream/stream edges. Value cross-rate edges
        // need `ValueLatchState` whose State has no `.kernel` field; the per-tick
        // emission later uses `.kernel.upsample(...)` and would fail to compile.
        // Value/event cross-rate edges fall back to the concrete-kernel emitter,
        // which uses `LatchUp`/`LatchDown` (value) or dedicated event drains.
        if !matches!(
            (edge.src_kind?, edge.dst_kind?),
            (EndpointKind::Stream, EndpointKind::Stream)
        ) {
            return None;
        }
        let src_ep = Self::ir_expr_as_endpoint(&edge.source)?;
        let (src_path, src_marker) = self.endpoint_marker_tokens(src_ep)?;
        let (dst_path, dst_marker) = self.endpoint_marker_tokens(&edge.dest)?;
        let (factor, dir, policy) = match edge.kernel {
            EdgeKernel::Up { factor, kind } => (
                factor,
                quote! { ::oscen::dispatch::UpDir },
                policy_marker_path(kind),
            ),
            EdgeKernel::Down { factor, kind } => (
                factor,
                quote! { ::oscen::dispatch::DownDir },
                policy_marker_path(kind),
            ),
            _ => return None,
        };
        Some(quote! {
            <() as ::oscen::dispatch::CrossRateKernel<
                <#src_path as ::oscen::dispatch::EndpointAt<#src_marker>>::Kind,
                <#dst_path as ::oscen::dispatch::EndpointAt<#dst_marker>>::Kind,
                #policy,
                #factor,
                #dir,
                <#src_path as ::oscen::dispatch::EndpointAt<#src_marker>>::Frame,
            >>::State
        })
    }

    // ========== Static Graph Generation ==========
    /// Extract the syn::Ident of the node referenced by a source expression.
    ///
    /// For a direct endpoint reference (`IrExprKind::Endpoint`) returns the
    /// node ident from the IR node table. For compound expressions (binary,
    /// method-call), descends into the left/receiver sub-expression to find
    /// the leftmost endpoint — preserving the pre-IR behaviour of the old
    /// `ConnectionExpr`-based helper. Returns `None` only for pure
    /// `Call` or `Literal` roots that contain no endpoint reference.
    fn extract_root_node<'e>(
        &'e self,
        expr: &'e crate::ir::expr::IrExpr,
    ) -> Option<&'e syn::Ident> {
        use crate::ir::expr::IrExprKind;
        match &expr.kind {
            IrExprKind::Endpoint(ep) => Some(&self.ir.nodes[ep.node].name),
            IrExprKind::Binary { left, .. } => self.extract_root_node(left),
            IrExprKind::MethodCall { receiver, .. } => self.extract_root_node(receiver),
            IrExprKind::Call { .. } | IrExprKind::Literal(_) => None,
        }
    }

    /// True iff the expression is a pure endpoint reference (no arithmetic,
    /// no function or method calls).
    fn is_simple_endpoint_source(expr: &crate::ir::expr::IrExpr) -> bool {
        matches!(expr.kind, crate::ir::expr::IrExprKind::Endpoint(_))
    }

    /// Extract the endpoint field name from a source expression.
    ///
    /// For a direct endpoint reference returns the field ident, **unless** the
    /// endpoint was lowered from a bare `ConnectionExpr::Ident` (graph input
    /// accessed without a dot-field selector), in which case returns `None`.
    /// For compound expressions descends into the left/receiver (leftmost-first)
    /// to find the first endpoint's field. Returns `None` for pure
    /// `Call`/`Literal`.
    fn extract_endpoint_field<'e>(
        &'e self,
        expr: &'e crate::ir::expr::IrExpr,
    ) -> Option<&'e syn::Ident> {
        use crate::ir::expr::IrExprKind;
        match &expr.kind {
            IrExprKind::Endpoint(ep) => {
                if ep.bare {
                    None
                } else {
                    Some(&ep.endpoint)
                }
            }
            IrExprKind::Binary { left, .. } => self.extract_endpoint_field(left),
            IrExprKind::MethodCall { receiver, .. } => self.extract_endpoint_field(receiver),
            IrExprKind::Call { .. } | IrExprKind::Literal(_) => None,
        }
    }

    /// Emit a TokenStream that evaluates the expression at runtime.
    ///
    /// For `Endpoint` variants, uses the IR's resolved node name + endpoint
    /// field. Replaces the AST-walking `connection_expr_to_tokens`.
    fn emit_expr(&self, expr: &crate::ir::expr::IrExpr) -> TokenStream {
        use crate::ir::expr::IrExprKind;
        match &expr.kind {
            IrExprKind::Endpoint(ep) => self.emit_endpoint(ep),
            IrExprKind::Binary { left, op, right } => {
                let l = self.emit_expr(left);
                let r = self.emit_expr(right);
                let op_token = match op {
                    BinaryOp::Add => quote! { + },
                    BinaryOp::Sub => quote! { - },
                    BinaryOp::Mul => quote! { * },
                    BinaryOp::Div => quote! { / },
                };
                quote! { (#l #op_token #r) }
            }
            IrExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv = self.emit_expr(receiver);
                quote! { #recv.#method(#(#args),*) }
            }
            IrExprKind::Call { function, args } => {
                let arg_tokens: Vec<_> = args.iter().map(|a| self.emit_expr(a)).collect();
                // A call whose path ends in `Frame` (bare or qualified, e.g.
                // `frame::Frame(a, b)`) is a frame constructor from scalar
                // channels. `Frame<N>` is a tuple struct over `[f32; N]`, so
                // wrap the channel args in an array literal; width is inferred
                // from the arg count and the destination type.
                let is_frame_ctor = function.segments.last().is_some_and(|s| s.ident == "Frame");
                if is_frame_ctor {
                    quote! { ::oscen::frame::Frame([#(#arg_tokens),*]) }
                } else {
                    quote! { #function(#(#arg_tokens),*) }
                }
            }
            IrExprKind::Literal(lit) => quote! { #lit },
        }
    }

    /// Emit tokens for an `IrEndpoint` reference (`self.osc.output` or
    /// `self.voices[3].output`). For bare-ident references (graph input/output
    /// nodes lowered from `ConnectionExpr::Ident`), emits just `self.<name>`.
    fn emit_endpoint(&self, ep: &crate::ir::expr::IrEndpoint) -> TokenStream {
        let node_name = &self.ir.nodes[ep.node].name;
        let endpoint_name = &ep.endpoint;
        match ep.index {
            // An index on a node-array element selects the element; an index on a
            // scalar node's endpoint selects a channel of its `Frame<N>` value.
            Some(idx) if self.get_node_array_size(node_name).is_some() => {
                quote! { self.#node_name[#idx].#endpoint_name }
            }
            Some(idx) => quote! { self.#node_name.#endpoint_name.0[#idx] },
            None => {
                if ep.bare {
                    // A ramped graph value input is stored as a
                    // `ValueRampState`; expressions read its `.current` f32.
                    if self.is_ramped_input(node_name).is_some() {
                        quote! { self.#node_name.current }
                    } else {
                        quote! { self.#node_name }
                    }
                } else {
                    quote! { self.#node_name.#endpoint_name }
                }
            }
        }
    }

    /// Generate the shared process body: connection assignments, node processing,
    /// and output routing.
    fn generate_process_body(&self) -> Result<Vec<TokenStream>> {
        let sorted_nodes: Vec<syn::Ident> = self.nodes().map(|n| n.name.clone()).collect();

        let mut process_body = Vec::new();

        for node_name in &sorted_nodes {
            let assignments = self.generate_connection_assignments_for_node(node_name);
            process_body.extend(assignments);

            process_body.push(self.emit_node_process_call(node_name));
        }

        process_body.extend(self.generate_graph_output_assignments_filtered(|_| true));

        Ok(process_body)
    }

    /// Generate event queue clearing statements for graph-level event inputs.
    fn generate_event_input_clearing(&self) -> Vec<TokenStream> {
        let mut clearing = Vec::new();
        for node in self.inputs() {
            let name = &node.name;
            if matches!(self.input_kind(name), Some(EndpointKind::Event)) {
                clearing.push(quote! {
                    self.#name.clear();
                });
            }
        }
        clearing
    }

    /// Generate event queue clearing statements for graph-level event outputs.
    /// Outputs are cleared at the START of a processing cycle (not after it),
    /// so events produced during the cycle stay readable by the host / an
    /// outer graph until the next cycle begins.
    fn generate_event_output_clearing(&self) -> Vec<TokenStream> {
        let mut clearing = Vec::new();
        for node in self.outputs() {
            let name = &node.name;
            if matches!(self.output_kind(name), Some(EndpointKind::Event)) {
                clearing.push(quote! {
                    self.#name.clear();
                });
            }
        }
        clearing
    }

    /// Generate the static process() method for compile-time graphs.
    /// The per-frame computation itself lives in the shared `__frame_core`
    /// (also called by `__advance_one_frame`); this wrapper only adds the
    /// per-cycle event queue discipline.
    fn generate_static_process(&self) -> Result<TokenStream> {
        let event_input_clearing = self.generate_event_input_clearing();
        let event_output_clearing = self.generate_event_output_clearing();

        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) {
                // Clear event outputs from the previous cycle
                #(#event_output_clearing)*

                self.__frame_core();

                // Clear event inputs after processing (outputs stay readable
                // until the next cycle)
                #(#event_input_clearing)*
            }
        })
    }

    /// Generate event handler methods for static graphs.
    fn generate_static_event_handler_methods(&self) -> Vec<TokenStream> {
        let mut methods = Vec::new();

        for node in self.inputs() {
            let endpoint_name = &node.name;
            if !matches!(self.input_kind(endpoint_name), Some(EndpointKind::Event)) {
                continue;
            }
            let method_name = syn::Ident::new(
                &format!("handle_{}_events", ident_base(endpoint_name)),
                endpoint_name.span(),
            );

            methods.push(quote! {
                pub fn #method_name(
                    &mut self,
                    events: &::oscen::graph::StaticEventQueue,
                ) {
                    // Copy events to this graph's input queue
                    // process() will route them to internal nodes
                    self.#endpoint_name.clear();
                    for event in events.iter() {
                        ::oscen::graph::debug_assert_event_pushed(
                            self.#endpoint_name.try_push(event.clone()),
                        );
                    }
                }
            });
        }

        methods
    }

    /// Generate get_stream_output() method for static graphs
    fn generate_static_get_stream_output(&self) -> TokenStream {
        let mut match_arms = Vec::new();
        let mut output_idx = 0usize;

        for node in self.outputs() {
            let field_name = &node.name;
            if !matches!(self.output_kind(field_name), Some(EndpointKind::Stream)) {
                continue;
            }
            match_arms.push(quote! {
                #output_idx => Some(self.#field_name)
            });
            output_idx += 1;
        }

        // The accessor returns the graph's stream frame type (`f32` for mono;
        // `Frame<N>` for an all-`Frame<N>` graph). Mixed graphs are out of scope
        // (their `BlockRender` impl already `compile_error!`s); fall back to f32.
        let frame_ty = match self.block_render_frame_ty() {
            BlockFrameTy::Frame(ty) => quote! { #ty },
            _ => quote! { f32 },
        };

        quote! {
            #[inline(always)]
            pub fn get_stream_output(&self, index: usize) -> Option<#frame_ty> {
                match index {
                    #(#match_arms,)*
                    _ => None
                }
            }
        }
    }

    /// Generate `impl BlockRender<F>` so the graph supports offline rendering,
    /// where `F` is the single frame type shared by all stream endpoints.
    fn generate_block_render_impl(&self, name: &syn::Ident) -> TokenStream {
        // A graph whose stream endpoints mix frame types cannot be rendered
        // offline (one `BlockRender<F>` cannot serve both). The graph still
        // works in realtime; only offline rendering is gated.
        // `impl BlockRender for G` (mono) keeps the trait's default `F = f32`,
        // byte-identical to the pre-generalization codegen; frame graphs spell
        // out `impl BlockRender<Frame<N>> for G`.
        let (trait_ref, frame_ty) = match self.block_render_frame_ty() {
            BlockFrameTy::Mono => (quote! { ::oscen::graph::BlockRender }, quote! { f32 }),
            BlockFrameTy::Frame(ty) => {
                (quote! { ::oscen::graph::BlockRender<#ty> }, quote! { #ty })
            }
            BlockFrameTy::Mixed => {
                return quote! {
                    ::core::compile_error!(
                        "offline `BlockRender` requires all of a graph's stream \
                         endpoints to share one frame type; this graph mixes `f32` \
                         and `Frame<N>` (or differing `Frame<N>`) stream endpoints"
                    );
                };
            }
        };

        let mut input_arms = Vec::new();
        let mut n_in = 0usize;
        for node in self.inputs() {
            let field_name = &node.name;
            if !matches!(self.input_kind(field_name), Some(EndpointKind::Stream)) {
                continue;
            }
            let block_name = block_field_name(field_name);
            input_arms.push(quote! { #n_in => &mut self.#block_name });
            n_in += 1;
        }

        let mut output_arms = Vec::new();
        let mut n_out = 0usize;
        for node in self.outputs() {
            let field_name = &node.name;
            if !matches!(self.output_kind(field_name), Some(EndpointKind::Stream)) {
                continue;
            }
            let block_name = block_field_name(field_name);
            output_arms.push(quote! { #n_out => &self.#block_name });
            n_out += 1;
        }

        quote! {
            impl #trait_ref for #name {
                const NUM_STREAM_INPUTS: usize = #n_in;
                const NUM_STREAM_OUTPUTS: usize = #n_out;

                #[inline]
                fn run_block(&mut self, frames: usize) {
                    self.process_block(frames);
                }

                fn stream_input_block_mut(&mut self, index: usize) -> &mut [#frame_ty] {
                    match index {
                        #(#input_arms,)*
                        _ => panic!("stream input index {} out of range", index),
                    }
                }

                fn stream_output_block(&self, index: usize) -> &[#frame_ty] {
                    match index {
                        #(#output_arms,)*
                        _ => panic!("stream output index {} out of range", index),
                    }
                }
            }
        }
    }

    /// Generate clear_event_outputs() method for graph types.
    fn generate_static_clear_event_outputs(&self) -> TokenStream {
        let mut clear_stmts = Vec::new();

        for node in self.outputs() {
            let name = &node.name;
            if matches!(self.output_kind(name), Some(EndpointKind::Event)) {
                clear_stmts.push(quote! {
                    self.#name.clear();
                });
            }
        }

        quote! {
            /// Clear all event outputs before handlers run.
            /// Called by outer graphs when this graph is used as a nested node.
            #[inline]
            pub fn clear_event_outputs(&mut self) {
                #(#clear_stmts)*
            }
        }
    }

    /// Generate `push_<name>()` helpers for graph-level event inputs.
    ///
    /// These are the supported way to inject events from the host/audio
    /// callback. `impl Into<EventPayload>` plus the `From<[u8; 3]>` /
    /// `From<f32>` conversions on `EventPayload` make the allocation-free
    /// representations the path of least resistance:
    /// `graph.push_midi_in([0x90, 60, 100], 0)`.
    fn generate_event_push_methods(&self) -> Vec<TokenStream> {
        self.inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Event)))
            .map(|node| {
                let name = &node.name;
                let push_name = syn::Ident::new(&format!("push_{}", ident_base(name)), name.span());
                let doc = format!(
                    "Push an event into the `{name}` event input for the next \
                     `process_block` call. `frame_offset` is relative to the \
                     start of that block. Allocation-free for scalar and raw \
                     MIDI payloads (`f32` / `[u8; 3]`). Returns `false` if the \
                     queue is full (the event is dropped)."
                );
                quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #push_name(
                        &mut self,
                        payload: impl Into<::oscen::graph::EventPayload>,
                        frame_offset: u32,
                    ) -> bool {
                        self.#name
                            .try_push(::oscen::graph::EventInstance {
                                frame_offset,
                                payload: payload.into(),
                            })
                            .is_ok()
                    }
                }
            })
            .collect()
    }

    /// Generate process_event_inputs() method for graph types.
    fn generate_static_process_event_inputs(&self) -> TokenStream {
        quote! {
            /// Process all event inputs: clear outputs before handlers run.
            /// Called by outer graphs when this graph is used as a nested node.
            /// The graph-level event inputs get routed to internal nodes during process().
            #[inline]
            pub fn process_event_inputs(&mut self) {
                self.clear_event_outputs();
            }
        }
    }

    /// Generate the `process_block()` public method.
    fn generate_static_process_block(&self) -> Result<TokenStream> {
        let has_event_inputs = self
            .inputs()
            .any(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Event)));

        if !has_event_inputs {
            // No events: simple tight loop
            return Ok(quote! {
                /// Process a block of `frames` samples.
                /// Stream inputs should be written to `*_block` arrays before calling.
                /// Stream outputs will be available in `*_block` arrays after calling.
                pub fn process_block(&mut self, frames: usize) {
                    debug_assert!(frames <= Self::MAX_BLOCK_SIZE);
                    for __frame in 0..frames {
                        self.__advance_one_frame(__frame);
                    }
                }
            });
        }

        // Event inputs exist: generate sub-block splitting

        let event_inputs: Vec<&IrNode> = self
            .inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Event)))
            .collect();

        let staging: Vec<_> = event_inputs
            .iter()
            .map(|node| {
                let name = &node.name;
                let staged_name =
                    syn::Ident::new(&format!("__staged_{}", ident_base(name)), name.span());
                let cursor_name =
                    syn::Ident::new(&format!("__cursor_{}", ident_base(name)), name.span());
                quote! {
                    let mut #staged_name: ::oscen::graph::StaticEventQueue =
                        ::oscen::graph::StaticEventQueue::new();
                    // Skip the drain + sort entirely on the common
                    // no-events-this-block path.
                    if !self.#name.is_empty() {
                        for __e in self.#name.iter() {
                            ::oscen::graph::debug_assert_event_pushed(
                                #staged_name.try_push(__e.clone()),
                            );
                        }
                        self.#name.clear();
                        #staged_name.sort_unstable_by_key(|__e| __e.frame_offset);
                    }
                    let mut #cursor_name: usize = 0;
                }
            })
            .collect();

        let boundary_checks: Vec<_> = event_inputs
            .iter()
            .map(|node| {
                let name = &node.name;
                let staged_name =
                    syn::Ident::new(&format!("__staged_{}", ident_base(name)), name.span());
                let cursor_name =
                    syn::Ident::new(&format!("__cursor_{}", ident_base(name)), name.span());
                quote! {
                    if #cursor_name < #staged_name.len() {
                        __next_event = __next_event.min(
                            (#staged_name[#cursor_name].frame_offset as usize).max(__frame)
                        );
                    }
                }
            })
            .collect();

        let event_pushes: Vec<_> = event_inputs
            .iter()
            .map(|node| {
                let name = &node.name;
                let staged_name =
                    syn::Ident::new(&format!("__staged_{}", ident_base(name)), name.span());
                let cursor_name =
                    syn::Ident::new(&format!("__cursor_{}", ident_base(name)), name.span());
                quote! {
                    while #cursor_name < #staged_name.len()
                        && #staged_name[#cursor_name].frame_offset == __frame as u32
                    {
                        ::oscen::graph::debug_assert_event_pushed(
                            self.#name.try_push(#staged_name[#cursor_name].clone()),
                        );
                        #cursor_name += 1;
                    }
                }
            })
            .collect();

        let leftover_requeues: Vec<_> = event_inputs
            .iter()
            .map(|node| {
                let name = &node.name;
                let staged_name =
                    syn::Ident::new(&format!("__staged_{}", ident_base(name)), name.span());
                let cursor_name =
                    syn::Ident::new(&format!("__cursor_{}", ident_base(name)), name.span());
                quote! {
                    while #cursor_name < #staged_name.len() {
                        let mut __e = #staged_name[#cursor_name].clone();
                        __e.frame_offset -= frames as u32;
                        ::oscen::graph::debug_assert_event_pushed(
                            self.#name.try_push(__e),
                        );
                        #cursor_name += 1;
                    }
                }
            })
            .collect();

        let event_input_clearing = self.generate_event_input_clearing();

        Ok(quote! {
            /// Process a block of `frames` samples with sub-block splitting at event boundaries.
            /// Stream inputs should be written to `*_block` arrays before calling.
            /// Stream outputs will be available in `*_block` arrays after calling.
            /// Events should be pushed to event input queues with appropriate `frame_offset` values.
            /// Events whose `frame_offset` lands beyond `frames` are deferred: they stay queued
            /// with the offset rebased so they fire sample-accurately in a later block.
            pub fn process_block(&mut self, frames: usize) {
                debug_assert!(frames <= Self::MAX_BLOCK_SIZE);

                // Stage: copy events to local sorted storage, drain originals
                #(#staging)*

                let mut __frame: usize = 0;
                while __frame < frames {
                    // Find next event boundary across all event inputs
                    let mut __next_event: usize = frames;
                    #(#boundary_checks)*

                    // Tight loop up to next event boundary (no events, no branches)
                    while __frame < __next_event {
                        self.__advance_one_frame(__frame);
                        __frame += 1;
                    }

                    if __frame >= frames { break; }

                    // Push events at this boundary into graph-level queues
                    #(#event_pushes)*

                    // Process the event frame
                    self.__advance_one_frame(__frame);
                    __frame += 1;

                    // Clear event input queues so the next sub-block starts
                    // clean (event outputs are overwritten per frame by the
                    // output assignments and stay readable after the block)
                    #(#event_input_clearing)*
                }

                // The loop consumed every staged event with frame_offset < frames,
                // so anything left is beyond this block: re-queue it with the
                // offset rebased so it fires sample-accurately in a later block.
                #(#leftover_requeues)*
            }
        })
    }

    // ========== Value Ramp Methods ==========

    /// Generate tick_ramps() method.
    fn generate_tick_ramps_method(&self) -> TokenStream {
        let ramped: Vec<_> = self
            .inputs()
            .filter(|n| {
                matches!(self.input_kind(&n.name), Some(EndpointKind::Value))
                    && self.is_ramped_input(&n.name).is_some()
            })
            .map(|n| n.name.clone())
            .collect();

        if ramped.is_empty() {
            return quote! {
                #[inline(always)]
                fn tick_ramps(&mut self) {}
            };
        }

        let tick_stmts: Vec<_> = ramped
            .iter()
            .map(|name| {
                quote! {
                    if self.#name.tick() {
                        self.active_ramps -= 1;
                    }
                }
            })
            .collect();

        quote! {
            #[inline(always)]
            fn tick_ramps(&mut self) {
                if self.active_ramps > 0 {
                    #(#tick_stmts)*
                }
            }
        }
    }

    /// Generate setter methods for value inputs.
    fn generate_value_setter_methods(&self) -> Vec<TokenStream> {
        self.inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Value)))
            .map(|node| {
                let name = &node.name;
                let set_name = syn::Ident::new(&format!("set_{}", ident_base(name)), name.span());

                if let Some(default_frames) = self.is_ramped_input(name) {
                    let set_ramp_name = syn::Ident::new(
                        &format!("set_{}_with_ramp", ident_base(name)),
                        name.span(),
                    );
                    let set_immediate_name = syn::Ident::new(
                        &format!("set_{}_immediate", ident_base(name)),
                        name.span(),
                    );
                    quote! {
                        /// Set the value with the default ramp duration.
                        /// No-op if target is already the same (safe to call every frame).
                        #[inline]
                        pub fn #set_name(&mut self, value: f32) {
                            // Only start a new ramp if target actually changed
                            if value != self.#name.target {
                                if !self.#name.is_ramping() {
                                    self.active_ramps += 1;
                                }
                                self.#name.set_with_ramp(value, #default_frames as u32);
                            }
                        }

                        /// Set the value with a custom ramp duration in frames.
                        /// No-op if target is already the same (safe to call every frame).
                        #[inline]
                        pub fn #set_ramp_name(&mut self, value: f32, frames: u32) {
                            // Only start a new ramp if target actually changed
                            if value != self.#name.target {
                                if frames > 0 {
                                    if !self.#name.is_ramping() {
                                        self.active_ramps += 1;
                                    }
                                } else if self.#name.is_ramping() {
                                    // frames == 0 ends any in-flight ramp immediately.
                                    self.active_ramps -= 1;
                                }
                                self.#name.set_with_ramp(value, frames);
                            }
                        }

                        /// Set the value immediately without ramping.
                        #[inline]
                        pub fn #set_immediate_name(&mut self, value: f32) {
                            if self.#name.is_ramping() {
                                self.active_ramps -= 1;
                            }
                            self.#name.set_immediate(value);
                        }
                    }
                } else {
                    // TYPED value inputs take their declared type; plain
                    // params stay f32. Both are immediate field writes.
                    let value_ty = match self.typed_value_ty(name) {
                        Some(ty) => quote! { #ty },
                        None => quote! { f32 },
                    };
                    quote! {
                        /// Set the value immediately.
                        #[inline]
                        pub fn #set_name(&mut self, value: #value_ty) {
                            self.#name = value;
                        }
                    }
                }
            })
            .collect()
    }

    // ========== NIH-plug Parameter Generation ==========

    /// Generate the NIH-plug params struct and its implementations
    fn generate_nih_params_struct(&self, graph_name: &syn::Ident) -> TokenStream {
        let params_name = syn::Ident::new(
            &format!("{}Params", ident_base(graph_name)),
            graph_name.span(),
        );

        // Collect value inputs for parameter generation. TYPED value inputs
        // are excluded: they are not DAW parameters (no FloatParam, no
        // sync_to entry) — hosts drive them through the typed setter. This
        // is the same filtered, same-ordered list `generate_param_registry`
        // uses, which keeps the positional `param_descriptors()[idx]`
        // lookups below aligned with the descriptor table.
        let value_inputs: Vec<&IrNode> = self.param_value_inputs();

        // Generate field definitions
        let param_fields: Vec<_> = value_inputs
            .iter()
            .map(|node| {
                let field_name = &node.name;
                let id_string = field_name.to_string();
                quote! {
                    #[id = #id_string]
                    pub #field_name: ::nih_plug::prelude::FloatParam
                }
            })
            .collect();

        // Generate Default impl with FloatParam constructors
        let param_defaults: Vec<_> = value_inputs.iter().enumerate().map(|(idx, node)| {
            let field_name = &node.name;
            let spec = self.input_spec(node);
            let display_name = spec
                .and_then(|s| s.display_name.clone())
                .unwrap_or_else(|| helpers::title_case(&field_name.to_string()));

            // Single source of truth: the graph's descriptor table, which
            // also resolves hoist-inherited defaults (inputs without an
            // explicit `= default` that start at the child constructor's
            // value). `value_inputs` here uses the same filter and order as
            // `generate_param_registry`, so the indices line up.
            let default_val = quote! { #graph_name::param_descriptors()[#idx].default };

            // Build the FloatRange
            let range_expr = if let Some(spec) = spec {
                if let Some(range) = &spec.range {
                    let min = &range.min;
                    let max = &range.max;
                    if let Some(center) = &spec.center {
                        // Calculate skew factor so that `center` is at normalized 0.5
                        quote! {
                            ::nih_plug::prelude::FloatRange::Skewed {
                                min: #min,
                                max: #max,
                                factor: 0.5f32.log((#center - #min) / (#max - #min)),
                            }
                        }
                    } else {
                        quote! {
                            ::nih_plug::prelude::FloatRange::Linear {
                                min: #min,
                                max: #max,
                            }
                        }
                    }
                } else {
                    quote! {
                        ::nih_plug::prelude::FloatRange::Linear {
                            min: 0.0,
                            max: 1.0,
                        }
                    }
                }
            } else {
                quote! {
                    ::nih_plug::prelude::FloatRange::Linear {
                        min: 0.0,
                        max: 1.0,
                    }
                }
            };

            // Build the FloatParam with optional modifiers
            let mut param_builder = quote! {
                ::nih_plug::prelude::FloatParam::new(
                    #display_name,
                    #default_val,
                    #range_expr,
                )
            };

            // Add smoother only if explicitly requested via `smoother:` attribute.
            let is_ramped = self.is_ramped_input(field_name).is_some();
            if !is_ramped {
                let smoother_ms = spec.and_then(|s| s.smoother.clone());
                if let Some(smoother_val) = smoother_ms {
                    param_builder = quote! {
                        #param_builder
                            .with_smoother(::nih_plug::prelude::SmoothingStyle::Linear(#smoother_val))
                    };
                }
            }

            // Add optional unit
            if let Some(spec) = spec {
                if let Some(unit) = &spec.unit {
                    let unit_with_space = format!(" {}", unit);
                    param_builder = quote! {
                        #param_builder
                            .with_unit(#unit_with_space)
                    };
                }

                // Add optional step size
                if let Some(step) = &spec.step {
                    param_builder = quote! {
                        #param_builder
                            .with_step_size(#step)
                    };
                }
            }

            quote! {
                #field_name: #param_builder
            }
        }).collect();

        // Generate sync_to method
        let sync_assignments: Vec<_> = value_inputs
            .iter()
            .map(|node| {
                let field_name = &node.name;
                let set_name = syn::Ident::new(
                    &format!("set_{}", ident_base(field_name)),
                    field_name.span(),
                );
                if self.is_ramped_input(field_name).is_some() {
                    quote! {
                        graph.#set_name(self.#field_name.value());
                    }
                } else {
                    quote! {
                        graph.#field_name = self.#field_name.value();
                    }
                }
            })
            .collect();

        quote! {
            #[derive(::nih_plug::prelude::Params)]
            pub struct #params_name {
                #(#param_fields),*
            }

            impl Default for #params_name {
                fn default() -> Self {
                    Self {
                        #(#param_defaults),*
                    }
                }
            }

            impl #params_name {
                /// Sync parameter values to the graph (call once per block)
                #[inline(always)]
                pub fn sync_to(&self, graph: &mut #graph_name) {
                    #(#sync_assignments)*
                }
            }
        }
    }

    /// Check if this graph has any ramped inputs
    fn has_ramped_inputs(&self) -> bool {
        self.inputs().any(|n| {
            matches!(self.input_kind(&n.name), Some(EndpointKind::Value))
                && self.is_ramped_input(&n.name).is_some()
        })
    }

    /// Collect every field of the generated struct, in declaration order:
    /// `sample_rate` (+ `active_ramps`), inputs (+ stream block buffers),
    /// outputs (+ stream block buffers), node instances, asset load handles.
    /// Resampler fields are appended separately by the caller.
    fn collect_struct_fields(&self) -> Vec<TokenStream> {
        let mut fields = vec![quote! { sample_rate: f32 }];

        // Add active_ramps counter if there are ramped inputs
        if self.has_ramped_inputs() {
            fields.push(quote! { active_ramps: u32 });
        }

        // Add input fields
        for node in self.inputs() {
            let field_name = &node.name;
            let kind = self.input_kind(field_name).unwrap_or(EndpointKind::Value);
            let ty = match kind {
                EndpointKind::Value => {
                    if let Some(ty) = self.typed_value_ty(field_name) {
                        // TYPED value input: a real field of the declared type.
                        quote! { #ty }
                    } else if self.is_ramped_input(field_name).is_some() {
                        quote! { ::oscen::graph::ValueRampState }
                    } else {
                        quote! { f32 }
                    }
                }
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
                EndpointKind::Stream => self.stream_field_ty(field_name),
                // Assets are externals, not graph inputs — never reached here.
                EndpointKind::Asset => unreachable!("asset endpoint is not a graph input"),
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream inputs (typed to the endpoint's frame type)
            if kind == EndpointKind::Stream {
                let block_name = block_field_name(field_name);
                let frame_ty = self.stream_field_ty(field_name);
                fields.push(
                    quote! { pub #block_name: [#frame_ty; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] },
                );
            }
        }

        // Add output fields (store actual values for static graphs)
        for node in self.outputs() {
            let field_name = &node.name;
            let kind = self.output_kind(field_name).unwrap_or(EndpointKind::Stream);
            let ty = match kind {
                EndpointKind::Stream => self.stream_field_ty(field_name),
                EndpointKind::Value => match self.typed_value_ty(field_name) {
                    // TYPED value output: a real field of the declared type.
                    Some(ty) => quote! { #ty },
                    None => quote! { f32 },
                },
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
                // Assets are externals, not graph outputs — never reached here.
                EndpointKind::Asset => unreachable!("asset endpoint is not a graph output"),
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream outputs (typed to the endpoint's frame type)
            if kind == EndpointKind::Stream {
                let block_name = block_field_name(field_name);
                let frame_ty = self.stream_field_ty(field_name);
                fields.push(
                    quote! { pub #block_name: [#frame_ty; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] },
                );
            }
        }

        // Add concrete node fields (no IO structs)
        for node in self.nodes() {
            let field_name = &node.name;
            if let Some(node_type) = self.node_type_path(node) {
                let array_size = match &node.kind {
                    IrNodeKind::NodeArray { len, .. } => Some(*len),
                    _ => None,
                };
                if let Some(array_size) = array_size {
                    // Array of nodes
                    fields.push(quote! { pub #field_name: [#node_type; #array_size] });
                } else {
                    // Single node
                    fields.push(quote! { pub #field_name: #node_type });
                }
            }
        }

        // Asset load-handle fields (one per `external -> node.asset` binding).
        fields.extend(self.generate_asset_handle_fields());

        fields
    }

    /// Assemble the complete generated item set: struct declaration,
    /// inherent impl (constructor, process entry points, setters, params),
    /// and trait impls. Every constituent comes from a dedicated
    /// `generate_*` / `collect_*` method; this is pure orchestration.
    fn generate_static_struct(&self) -> Result<TokenStream> {
        let name = self.name();
        let fields = self.collect_struct_fields();

        let input_params = self.generate_static_input_params();
        let hoist_inherits = self.generate_hoist_default_inherits();
        let output_params = self.generate_static_output_params();
        let node_init = self.generate_static_node_init();
        let asset_wiring = self.generate_asset_wiring();
        let asset_set_rate_calls = self.generate_asset_set_graph_rate_calls();
        let struct_init = self.generate_static_struct_init();

        let resampler_fields = self.generate_resampler_fields();
        let resampler_inits = self.generate_resampler_inits();

        let kind_assertions = self.generate_kind_assertions();
        let feedback_assertions = self.generate_feedback_assertions();

        // For compile-time graphs, generate a static process() method
        let frame_core_method = self.generate_frame_core()?;
        let process_method = self.generate_static_process()?;
        let advance_one_frame_method = self.generate_advance_one_frame()?;
        let process_block_method = self.generate_static_process_block()?;
        let get_stream_output_method = self.generate_static_get_stream_output();
        let block_render_impl = self.generate_block_render_impl(name);
        let clear_event_outputs_method = self.generate_static_clear_event_outputs();
        let process_event_inputs_method = self.generate_static_process_event_inputs();
        let event_push_methods = self.generate_event_push_methods();
        let event_handler_methods = self.generate_static_event_handler_methods();
        let tick_ramps_method = self.generate_tick_ramps_method();
        let value_setter_methods = self.generate_value_setter_methods();
        let latency_method = self.generate_latency_method();

        let node_prepare_calls = self.generate_node_prepare_calls();
        let node_set_rate_calls = self.generate_node_set_sample_rate_calls();
        let resampler_resets = self.generate_resampler_resets();

        // Parameter registry: id enum + descriptor table + dispatchers.
        let param_registry = self.generate_param_registry()?;

        // Generate NIH-plug params struct if nih_params flag is set
        let nih_params_output = if self.nih_params() {
            self.generate_nih_params_struct(name)
        } else {
            quote! {}
        };

        // Endpoint manifest macro: lets a parent `graph!` enumerate this
        // graph's endpoints at expansion time (wildcard hoists through
        // nested graphs). Mirrors the manifest `#[derive(Node)]` emits.
        let endpoint_manifest = self.generate_endpoint_manifest();

        // If there are any cross-rate edges we append a leading comma to the
        // tail so the existing `#struct_init` (which has no trailing comma)
        // chains cleanly into the resampler inits.
        let resampler_init_tail = if resampler_inits.is_empty() {
            quote! {}
        } else {
            quote! { , #(#resampler_inits),* }
        };

        Ok(quote! {
            #(#kind_assertions)*

            #(#feedback_assertions)*

            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #name {
                #(#fields,)*
                #(#resampler_fields,)*
            }

            impl #name {
                /// Maximum block size for `process_block()`.
                pub const MAX_BLOCK_SIZE: usize = ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE;

                #[allow(unused_variables, unused_mut)]
                pub fn new() -> Self {
                    let sample_rate = 44100.0; // Default sample rate, will be set via init()

                    // Initialize input parameters
                    #(#input_params)*

                    // Initialize output parameters
                    #(#output_params)*

                    // Initialize nodes (direct instantiation)
                    #(#node_init)*

                    // Wire up asset load handles (handoff pair + install).
                    #(#asset_wiring)*

                    // Hoisted inputs without an explicit `= default` inherit
                    // their initial value from the child node they hoist.
                    #(#hoist_inherits)*

                    Self {
                        #struct_init
                        #resampler_init_tail
                    }
                }

                /// Set the graph's sample rate and propagate it to every child
                /// node (scaled by each node's rate annotation, recursing into
                /// nested graphs). Rate only: unlike `init`, this does not
                /// reset resamplers or recompute derived state.
                #[inline]
                pub fn set_sample_rate(&mut self, sample_rate: f32) {
                    self.sample_rate = sample_rate;
                    #(#node_set_rate_calls)*
                    // Record the graph rate on each asset load handle so a
                    // subsequent `load_wav` validates against the right rate.
                    #(#asset_set_rate_calls)*
                }

                /// Host entry point: distribute `sample_rate` to every node
                /// and prepare the graph for processing. Equivalent to
                /// `set_sample_rate(sample_rate)` followed by
                /// `SignalProcessor::prepare`.
                pub fn init(&mut self, sample_rate: f32) {
                    self.set_sample_rate(sample_rate);
                    ::oscen::SignalProcessor::prepare(self);
                }

                #frame_core_method

                #process_method

                #advance_one_frame_method

                #process_block_method

                #get_stream_output_method

                #clear_event_outputs_method

                #process_event_inputs_method

                #(#event_push_methods)*

                #(#event_handler_methods)*

                #tick_ramps_method

                #(#value_setter_methods)*

                #latency_method
            }

            // Generate SignalProcessor implementation for compile-time graphs
            impl ::oscen::SignalProcessor for #name {
                fn prepare(&mut self) {
                    // Rates were already distributed by set_sample_rate (the
                    // parent graph or the inherent init() calls it first).
                    // Prepare every child node.
                    #(#node_prepare_calls)*
                    // Reset every cross-rate resampler kernel.
                    #(#resampler_resets)*
                }

                fn process(&mut self) {
                    // This is already implemented in the impl block above
                }
            }

            #block_render_impl

            #param_registry

            #nih_params_output

            #endpoint_manifest
        })
    }

    /// Emit the endpoint-manifest macro for this graph type: an exported
    /// `macro_rules!` (`__oscen_endpoints_<GraphName>!`) that invokes a
    /// caller-supplied continuation with the graph's endpoint list
    /// appended to arbitrary passthrough state. Inputs are the graph's
    /// declared inputs (including expanded hoists); outputs are the
    /// declared outputs. This is what lets a parent graph write
    /// `input nested.*;` where `nested` is itself a `graph!` type.
    ///
    /// Emission is shared with `#[derive(Node)]` via
    /// [`crate::manifest::emit_manifest_export`]: the `#[macro_export]`
    /// name is mangled (`__oscen_endpoints_export_<Name>_<hash>`, hashing
    /// the graph body so same-named graphs don't collide) with a
    /// `pub use … as …` re-export next to the type, so qualified manifest
    /// paths mirror the graph type's own path. `graph!` invoked inside a
    /// function body works too (`#[macro_export]` still exports at the
    /// crate root; the local re-export is allowed but only usable in that
    /// scope).
    fn generate_endpoint_manifest(&self) -> TokenStream {
        use crate::manifest::{ManifestEndpoint, ManifestRamp};
        // Annotate a graph endpoint with the metadata the manifest can
        // carry: the recognized frame type of stream endpoints (so
        // wildcard hoists preserve `Frame<2>` instead of collapsing to
        // mono f32) and the declared ramp length of ramped value inputs
        // (so wildcard hoists re-declare the same `[ramp: N]`).
        let annotate = |mut ep: ManifestEndpoint| {
            match ep.kind {
                EndpointKind::Stream => {
                    // Canonicalize to the fully-qualified path: manifest
                    // type tokens resolve at the consuming graph's call
                    // site, which need not have `Frame`/`Stereo`/… in
                    // scope.
                    ep.ty = self
                        .endpoint_frame_ty(&ep.name)
                        .map(|ty| qualified_frame_ty(&ty));
                }
                EndpointKind::Value => {
                    if let Some(ty) = self.typed_value_ty(&ep.name) {
                        // TYPED value endpoint: carry the declared type's
                        // literal tokens (mirroring the stream branch; user
                        // types can't be canonicalized, so the tokens
                        // resolve at the consuming graph's call site — same
                        // hygiene caveat as `#[derive(Node)]` manifests).
                        ep.ty = Some(ty.clone());
                    } else {
                        if let Some(frames) = self.is_ramped_input(&ep.name) {
                            ep.ramp = ManifestRamp::Frames(frames);
                        }
                        // Carry the input's param-spec metadata so a parent
                        // wildcard hoist re-declares it intact (range/curve/
                        // unit/center/step/group/display). Expressions ride
                        // as raw tokens, resolving at the parent's call
                        // site — same hygiene caveat as `ty = …`.
                        if let Some(spec) = self
                            .find_node_by_ident(&ep.name)
                            .and_then(|node| self.input_spec(node))
                        {
                            ep.range = spec.range.as_ref().map(|r| (r.min.clone(), r.max.clone()));
                            ep.log = spec.curve == Some(crate::ast::Curve::Logarithmic);
                            ep.center = spec.center.clone();
                            ep.unit = spec.unit.clone();
                            ep.step = spec.step.clone();
                            ep.group = spec.group.clone();
                            ep.display_name = spec.display_name.clone();
                        }
                    }
                }
                EndpointKind::Event | EndpointKind::Asset => {}
            }
            ep
        };
        let inputs: Vec<ManifestEndpoint> = self
            .inputs()
            .map(|node| {
                let kind = self.input_kind(&node.name).unwrap_or(EndpointKind::Value);
                annotate(ManifestEndpoint::new(node.name.clone(), kind))
            })
            .collect();
        let outputs: Vec<ManifestEndpoint> = self
            .outputs()
            .map(|node| {
                let kind = self.output_kind(&node.name).unwrap_or(EndpointKind::Stream);
                annotate(ManifestEndpoint::new(node.name.clone(), kind))
            })
            .collect();
        crate::manifest::emit_manifest_export(self.name(), &inputs, &outputs, self.source_tokens)
    }
}

/// Canonicalize a recognized frame-type annotation (`Frame<2>`, `Stereo`,
/// `Mono`, `Quad` — see [`CodegenContext::endpoint_frame_ty`]) to its
/// fully-qualified `::oscen::frame::…` path, keeping any generic
/// arguments. Manifest `ty = …` tokens resolve at the consuming graph's
/// call site, so a bare `Frame` would require the parent to import it;
/// the qualified path always resolves. Unrecognized shapes are returned
/// unchanged (defensive — callers only pass recognized frame types).
fn qualified_frame_ty(ty: &syn::Type) -> syn::Type {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            let seg = seg.clone();
            return syn::parse_quote! { ::oscen::frame::#seg };
        }
    }
    ty.clone()
}

// Silence unused-import warnings for IR types pulled in for ergonomics.
#[allow(dead_code)]
fn _ir_types_in_use(_id: EdgeId) {}
