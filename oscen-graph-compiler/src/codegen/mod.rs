use crate::ast::{BinaryOp, EndpointKind, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::ir::graph::{EdgeId, EdgeKernel, IrEdge, IrGraph, IrNode, IrNodeKind, NodeId};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;
use syn::{Expr, Result};

mod helpers;
use helpers::*;

mod emit_edge;
mod emit_frame;
mod emit_node;
mod emit_struct;

pub fn generate(ir: &IrGraph) -> std::result::Result<TokenStream, Diagnostics> {
    // lower() has already run analysis + validation. Codegen consumes the
    // IR directly. Static graphs require a name (already enforced by
    // lower()), so we just emit.
    let ctx = CodegenContext::new(ir);
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
}

impl<'a> CodegenContext<'a> {
    fn new(ir: &'a IrGraph) -> Self {
        let mut name_to_id = HashMap::new();
        for (id, node) in &ir.nodes {
            name_to_id.insert(node.name.to_string(), id);
        }
        Self { ir, name_to_id }
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

    /// Infer the endpoint kind of an `IrExpr`.
    ///
    /// Thin facade over [`crate::ir::lower::endpoint_kind_of`], which is the
    /// single source of truth for endpoint-kind inference.
    fn infer_kind(&self, expr: &crate::ir::expr::IrExpr) -> Option<EndpointKind> {
        crate::ir::lower::endpoint_kind_of(expr, self.ir)
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
        let src_kind = self.infer_kind(&edge.source)?;
        let dst_kind = self.ir.nodes[edge.dest.node]
            .endpoints
            .get(&edge.dest.endpoint)
            .map(|ei| ei.kind)?;
        if !matches!(
            (src_kind, dst_kind),
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
                quote! { #function(#(#arg_tokens),*) }
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
            Some(idx) => quote! { self.#node_name[#idx].#endpoint_name },
            None => {
                if ep.bare {
                    quote! { self.#node_name }
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

    /// Generate event queue clearing statements for graph-level event inputs/outputs.
    fn generate_event_clearing(&self) -> Vec<TokenStream> {
        let mut clearing = Vec::new();
        for node in self.inputs() {
            let name = &node.name;
            if matches!(self.input_kind(name), Some(EndpointKind::Event)) {
                clearing.push(quote! {
                    self.#name.clear();
                });
            }
        }
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
    fn generate_static_process(&self) -> Result<TokenStream> {
        let event_clearing = self.generate_event_clearing();

        if self.max_factor() > 1 {
            // Multi-rate graph nested as a node: the multi-rate inner-loop
            // schedule must run on every call to `process()`.
            let body = self.generate_multirate_inner_body()?;
            return Ok(quote! {
                #[inline(always)]
                #[allow(unused_variables, unused_mut)]
                pub fn process(&mut self) {
                    #body

                    // Clear event queues after processing.
                    #(#event_clearing)*
                }
            });
        }

        let process_body = self.generate_process_body()?;
        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) {
                use ::oscen::SignalProcessor as _;

                // Advance ramped value inputs
                self.tick_ramps();

                #(#process_body)*

                // Clear event queues after processing
                #(#event_clearing)*
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
                &format!("handle_{}_events", endpoint_name),
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
                        let _ = self.#endpoint_name.try_push(event.clone());
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

        quote! {
            #[inline(always)]
            pub fn get_stream_output(&self, index: usize) -> Option<f32> {
                match index {
                    #(#match_arms,)*
                    _ => None
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
                let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
                let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
                quote! {
                    let mut #staged_name: ::oscen::graph::StaticEventQueue =
                        ::oscen::graph::StaticEventQueue::new();
                    for __e in self.#name.iter() {
                        let _ = #staged_name.try_push(__e.clone());
                    }
                    self.#name.clear();
                    #staged_name.sort_unstable_by_key(|__e| __e.frame_offset);
                    let mut #cursor_name: usize = 0;
                }
            })
            .collect();

        let boundary_checks: Vec<_> = event_inputs
            .iter()
            .map(|node| {
                let name = &node.name;
                let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
                let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
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
                let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
                let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
                quote! {
                    while #cursor_name < #staged_name.len()
                        && #staged_name[#cursor_name].frame_offset == __frame as u32
                    {
                        let _ = self.#name.try_push(#staged_name[#cursor_name].clone());
                        #cursor_name += 1;
                    }
                }
            })
            .collect();

        let event_clearing = self.generate_event_clearing();

        Ok(quote! {
            /// Process a block of `frames` samples with sub-block splitting at event boundaries.
            /// Stream inputs should be written to `*_block` arrays before calling.
            /// Stream outputs will be available in `*_block` arrays after calling.
            /// Events should be pushed to event input queues with appropriate `frame_offset` values.
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

                    // Clear event queues so next sub-block starts clean
                    #(#event_clearing)*
                }
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
                let set_name = syn::Ident::new(&format!("set_{}", name), name.span());

                if let Some(default_frames) = self.is_ramped_input(name) {
                    let set_ramp_name =
                        syn::Ident::new(&format!("set_{}_with_ramp", name), name.span());
                    let set_immediate_name =
                        syn::Ident::new(&format!("set_{}_immediate", name), name.span());
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
                                if frames > 0 && !self.#name.is_ramping() {
                                    self.active_ramps += 1;
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
                    quote! {
                        /// Set the value immediately.
                        #[inline]
                        pub fn #set_name(&mut self, value: f32) {
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
        let params_name = syn::Ident::new(&format!("{}Params", graph_name), graph_name.span());

        // Collect value inputs for parameter generation
        let value_inputs: Vec<&IrNode> = self
            .inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Value)))
            .collect();

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
        let param_defaults: Vec<_> = value_inputs.iter().map(|node| {
            let field_name = &node.name;
            let spec = self.input_spec(node);
            let display_name = spec
                .and_then(|s| s.display_name.clone())
                .unwrap_or_else(|| {
                    // Convert snake_case to Title Case
                    field_name.to_string()
                        .split('_')
                        .map(|word| {
                            let mut chars = word.chars();
                            match chars.next() {
                                None => String::new(),
                                Some(first) => first.to_uppercase().chain(chars).collect(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                });

            let default_val = self.input_default(node)
                .map(|expr| quote! { #expr })
                .unwrap_or_else(|| quote! { 0.0 });

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
                let set_name = syn::Ident::new(&format!("set_{}", field_name), field_name.span());
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

    fn generate_static_struct(&self) -> Result<TokenStream> {
        let name = self.name();
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
                    if self.is_ramped_input(field_name).is_some() {
                        quote! { ::oscen::graph::ValueRampState }
                    } else {
                        quote! { f32 }
                    }
                }
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
                EndpointKind::Stream => quote! { f32 },
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream inputs
            if kind == EndpointKind::Stream {
                let block_name =
                    syn::Ident::new(&format!("{}_block", field_name), field_name.span());
                fields.push(
                    quote! { pub #block_name: [f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] },
                );
            }
        }

        // Add output fields (store actual values for static graphs)
        for node in self.outputs() {
            let field_name = &node.name;
            let kind = self.output_kind(field_name).unwrap_or(EndpointKind::Stream);
            let ty = match kind {
                EndpointKind::Stream => quote! { f32 },
                EndpointKind::Value => quote! { f32 },
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream outputs
            if kind == EndpointKind::Stream {
                let block_name =
                    syn::Ident::new(&format!("{}_block", field_name), field_name.span());
                fields.push(
                    quote! { pub #block_name: [f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] },
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

        let input_params = self.generate_static_input_params();
        let output_params = self.generate_static_output_params();
        let node_init = self.generate_static_node_init();
        let struct_init = self.generate_static_struct_init();

        let resampler_fields = self.generate_resampler_fields();
        let resampler_inits = self.generate_resampler_inits();

        let kind_assertions = self.generate_kind_assertions();
        let feedback_assertions = self.generate_feedback_assertions();

        // For compile-time graphs, generate a static process() method
        let process_method = self.generate_static_process()?;
        let advance_one_frame_method = self.generate_advance_one_frame()?;
        let process_block_method = self.generate_static_process_block()?;
        let get_stream_output_method = self.generate_static_get_stream_output();
        let clear_event_outputs_method = self.generate_static_clear_event_outputs();
        let process_event_inputs_method = self.generate_static_process_event_inputs();
        let event_handler_methods = self.generate_static_event_handler_methods();
        let tick_ramps_method = self.generate_tick_ramps_method();
        let value_setter_methods = self.generate_value_setter_methods();
        let latency_method = self.generate_latency_method();

        let node_prepare_calls = self.generate_node_prepare_calls();
        let node_set_rate_calls = self.generate_node_set_sample_rate_calls();
        let resampler_resets = self.generate_resampler_resets();

        // Generate NIH-plug params struct if nih_params flag is set
        let nih_params_output = if self.nih_params() {
            self.generate_nih_params_struct(name)
        } else {
            quote! {}
        };

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
                }

                /// Host entry point: distribute `sample_rate` to every node
                /// and prepare the graph for processing. Equivalent to
                /// `set_sample_rate(sample_rate)` followed by
                /// `SignalProcessor::prepare`.
                pub fn init(&mut self, sample_rate: f32) {
                    self.set_sample_rate(sample_rate);
                    ::oscen::SignalProcessor::prepare(self);
                }

                #process_method

                #advance_one_frame_method

                #process_block_method

                #get_stream_output_method

                #clear_event_outputs_method

                #process_event_inputs_method

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

            #nih_params_output
        })
    }
}

// Silence unused-import warnings for IR types pulled in for ergonomics.
#[allow(dead_code)]
fn _ir_types_in_use(_id: EdgeId) {}
