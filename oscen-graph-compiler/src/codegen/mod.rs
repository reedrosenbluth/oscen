use crate::ast::{BinaryOp, EndpointKind, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::ir::graph::{
    EdgeId, EdgeKernel, EventRescale, FanoutShape, IrEdge, IrGraph, IrNode, IrNodeKind, NodeId,
};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::{HashMap, HashSet};
use syn::{Expr, Result};

mod helpers;
use helpers::*;

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

    // ========== Static Graph Parameter Generation ==========

    fn generate_static_input_params(&self) -> Vec<TokenStream> {
        self.inputs()
            .flat_map(|node| {
                let name = &node.name;
                let kind = node
                    .endpoints
                    .get(name)
                    .map(|e| e.kind)
                    .unwrap_or(EndpointKind::Value);
                let default_val = self.input_default(node);

                let mut stmts = Vec::new();
                match kind {
                    EndpointKind::Value => {
                        let default = default_val.map(|d| quote! { #d }).unwrap_or(quote! { 0.0 });
                        if self.is_ramped_input(name).is_some() {
                            stmts.push(quote! {
                                let #name = ::oscen::graph::ValueRampState::new(#default);
                            });
                        } else {
                            stmts.push(quote! {
                                let #name = #default;
                            });
                        }
                    }
                    EndpointKind::Event => {
                        stmts.push(quote! {
                            let #name = ::oscen::graph::StaticEventQueue::new();
                        });
                    }
                    EndpointKind::Stream => {
                        stmts.push(quote! {
                            let #name = 0.0f32;
                        });
                        // Block buffer for stream inputs
                        let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                        stmts.push(quote! {
                            let #block_name = [0.0f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE];
                        });
                    }
                }
                stmts
            })
            .collect()
    }

    /// Generate static initialization for output parameters
    /// For static graphs, outputs store actual values (f32) not endpoint wrappers
    fn generate_static_output_params(&self) -> Vec<TokenStream> {
        self.outputs()
            .flat_map(|node| {
                let name = &node.name;
                let kind = node
                    .endpoints
                    .get(name)
                    .map(|e| e.kind)
                    .unwrap_or(EndpointKind::Stream);
                let mut stmts = Vec::new();

                match kind {
                    EndpointKind::Stream => {
                        stmts.push(quote! {
                            let #name = 0.0f32;
                        });
                        // Block buffer for stream outputs
                        let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                        stmts.push(quote! {
                            let #block_name = [0.0f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE];
                        });
                    }
                    EndpointKind::Value => {
                        stmts.push(quote! {
                            let #name = 0.0f32;
                        });
                    }
                    EndpointKind::Event => {
                        stmts.push(quote! {
                            let #name = ::oscen::graph::StaticEventQueue::new();
                        });
                    }
                }
                stmts
            })
            .collect()
    }

    /// Generate static initialization for nodes (direct constructor calls)
    fn generate_static_node_init(&self) -> Vec<TokenStream> {
        self.nodes()
            .map(|node| {
                let name = &node.name;
                let constructor_expr = self
                    .node_ctor_expr(node)
                    .expect("processor/array node must have a constructor expression");
                // For static graphs:
                // - If constructor is a path (Type), call Type::new() (Pattern 2)
                // - If constructor is already a call, use it as-is
                let constructor = match constructor_expr {
                    Expr::Path(path) => {
                        // Pattern 2: call new() without arguments
                        // init(sample_rate) will be called later
                        quote! { #path::new() }
                    }
                    Expr::Call(_) => {
                        quote! { #constructor_expr }
                    }
                    _ => {
                        quote! { #constructor_expr }
                    }
                };

                let array_size = match &node.kind {
                    IrNodeKind::NodeArray { len, .. } => Some(*len),
                    _ => None,
                };

                if let Some(array_size) = array_size {
                    // Generate array initialization by repeating constructor
                    let constructors = vec![constructor.clone(); array_size];
                    quote! {
                        let #name = [#(#constructors),*];
                    }
                } else {
                    // Single node initialization
                    quote! {
                        let #name = #constructor;
                    }
                }
            })
            .collect()
    }

    /// Generate static struct initialization (includes sample_rate, nodes - no IO fields)
    fn generate_static_struct_init(&self) -> TokenStream {
        let has_ramped = self.has_ramped_inputs();

        let active_ramps_init = if has_ramped {
            quote! { active_ramps: 0, }
        } else {
            quote! {}
        };

        // Add input/output fields (including block buffer fields for streams)
        let input_fields: Vec<_> = self
            .inputs()
            .flat_map(|node| {
                let name = &node.name;
                let kind = node
                    .endpoints
                    .get(name)
                    .map(|e| e.kind)
                    .unwrap_or(EndpointKind::Value);
                let mut fields = vec![quote! { #name }];
                if kind == EndpointKind::Stream {
                    let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                    fields.push(quote! { #block_name });
                }
                fields
            })
            .collect();

        let output_fields: Vec<_> = self
            .outputs()
            .flat_map(|node| {
                let name = &node.name;
                let kind = node
                    .endpoints
                    .get(name)
                    .map(|e| e.kind)
                    .unwrap_or(EndpointKind::Stream);
                let mut fields = vec![quote! { #name }];
                if kind == EndpointKind::Stream {
                    let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                    fields.push(quote! { #block_name });
                }
                fields
            })
            .collect();

        // Add node fields (no IO fields)
        let node_fields: Vec<_> = self
            .nodes()
            .map(|node| {
                let name = &node.name;
                quote! { #name }
            })
            .collect();

        // Note: Graph-level event storage is no longer generated
        // Nodes own their own EventInput/EventOutput storage

        quote! {
            sample_rate,
            #active_ramps_init
            #(#input_fields,)*
            #(#output_fields,)*
            #(#node_fields),*
        }
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
        let assoc_ident =
            syn::Ident::new(&format!("{}__Ep", ep.endpoint), proc_macro2::Span::call_site());
        Some((quote! { #path }, quote! { <#path>::#assoc_ident }))
    }

    /// Extract the `IrEndpoint` from an `IrExpr`, if the expression is a
    /// plain `Endpoint` variant. Returns `None` for compound expressions
    /// (Binary, MethodCall, Call, Literal).
    fn ir_expr_as_endpoint(
        expr: &crate::ir::expr::IrExpr,
    ) -> Option<&crate::ir::expr::IrEndpoint> {
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
            >>::State
        })
    }

    /// Emit a `quote_spanned!`-spanned const-time trait-bound assertion per
    /// cross-rate edge whose source and destination are both projectable.
    fn generate_kind_assertions(&self) -> Vec<TokenStream> {
        let mut out = Vec::new();
        for (_, edge) in self.edges() {
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
                EdgeKernel::None | EdgeKernel::Event { .. } => continue,
            };
            let src_ep = match Self::ir_expr_as_endpoint(&edge.source) {
                Some(ep) => ep,
                None => continue,
            };
            let (src_path, src_marker) = match self.endpoint_marker_tokens(src_ep) {
                Some(t) => t,
                None => continue,
            };
            let (dst_path, dst_marker) = match self.endpoint_marker_tokens(&edge.dest) {
                Some(t) => t,
                None => continue,
            };
            // Use the source expression's span so trait-resolution errors point
            // at the user's connection.
            let span = edge.source.span;
            let assertion = quote::quote_spanned! { span =>
                #[allow(non_snake_case)]
                const _: fn() = || {
                    fn _assert_supported<P, const __N: u32, Dir>()
                    where
                        (): ::oscen::dispatch::CrossRateKernel<
                            <#src_path as ::oscen::dispatch::EndpointAt<#src_marker>>::Kind,
                            <#dst_path as ::oscen::dispatch::EndpointAt<#dst_marker>>::Kind,
                            P, __N, Dir,
                        >,
                    {
                    }
                    _assert_supported::<#policy, #factor, #dir>();
                };
            };
            out.push(assertion);
        }
        out
    }

    /// Generate one struct field per cross-rate stream/value connection.
    fn generate_resampler_fields(&self) -> Vec<TokenStream> {
        let mut fields = Vec::new();
        for (idx, edge) in self.edges() {
            let ty = match self.cross_rate_kernel_state_type(edge) {
                Some(t) => t,
                None => match edge.kernel {
                    EdgeKernel::None | EdgeKernel::Event { .. } => continue,
                    EdgeKernel::Up { factor, kind } => kernel_up_type(factor, kind),
                    EdgeKernel::Down { factor, kind } => kernel_down_type(factor, kind),
                },
            };
            let field_name = resampler_field_name(idx);
            let field_ty = if let FanoutShape::Parallel { n } = edge.fanout {
                quote! { [#ty; #n] }
            } else {
                ty
            };
            fields.push(quote! { pub #field_name: #field_ty });
        }
        fields
    }

    /// Generate one initializer per cross-rate stream/value connection.
    fn generate_resampler_inits(&self) -> Vec<TokenStream> {
        let mut inits = Vec::new();
        for (idx, edge) in self.edges() {
            let projection = self.cross_rate_kernel_state_type(edge);
            let (ty_for_init, init_via_default) = match (&projection, edge.kernel) {
                (Some(t), _) => (t.clone(), true),
                (None, EdgeKernel::None) | (None, EdgeKernel::Event { .. }) => continue,
                (None, EdgeKernel::Up { factor, kind }) => (kernel_up_type(factor, kind), false),
                (None, EdgeKernel::Down { factor, kind }) => {
                    (kernel_down_type(factor, kind), false)
                }
            };
            let field_name = resampler_field_name(idx);
            let init_one = if init_via_default {
                quote! { <#ty_for_init as ::core::default::Default>::default() }
            } else {
                quote! { <#ty_for_init>::new() }
            };
            let init_expr = if matches!(edge.fanout, FanoutShape::Parallel { .. }) {
                quote! { ::core::array::from_fn(|_| #init_one) }
            } else {
                init_one
            };
            inits.push(quote! { #field_name: #init_expr });
        }
        inits
    }

    /// Generate per-node `init()` calls that scale `sample_rate` by the node's
    /// rate annotation.
    fn generate_node_init_calls_rate_aware(&self) -> Vec<TokenStream> {
        let mut calls = Vec::new();
        for node in self.nodes() {
            let name = &node.name;
            let scaled = match node.rate {
                NodeRate::Same => quote! { sample_rate },
                NodeRate::Up(f) => {
                    let f = f as f32;
                    quote! { sample_rate * #f }
                }
                NodeRate::Down(d) => {
                    let d = d as f32;
                    quote! { sample_rate / #d }
                }
            };
            let is_array = matches!(node.kind, IrNodeKind::NodeArray { .. });
            if is_array {
                calls.push(quote! {
                    for __child in self.#name.iter_mut() {
                        ::oscen::SignalProcessor::init(__child, #scaled);
                    }
                });
            } else {
                calls.push(quote! {
                    ::oscen::SignalProcessor::init(&mut self.#name, #scaled);
                });
            }
        }
        calls
    }

    /// Generate `reset()` calls for every cross-rate resampler kernel.
    fn generate_resampler_resets(&self) -> Vec<TokenStream> {
        let mut resets = Vec::new();
        for (idx, edge) in self.edges() {
            let f = resampler_field_name(idx);
            let projected = self.cross_rate_kernel_state_type(edge).is_some();
            let access = if projected {
                quote! { .kernel }
            } else {
                quote! {}
            };
            let reset_one = match edge.kernel {
                EdgeKernel::None | EdgeKernel::Event { .. } => continue,
                EdgeKernel::Up { .. } => quote! {
                    ::oscen::resample::StreamUpsampler::reset
                },
                EdgeKernel::Down { .. } => quote! {
                    ::oscen::resample::StreamDownsampler::reset
                },
            };
            let stmt = if let FanoutShape::Parallel { n } = edge.fanout {
                quote! {
                    for __k in 0..#n {
                        #reset_one(&mut self.#f[__k] #access);
                    }
                }
            } else {
                quote! { #reset_one(&mut self.#f #access); }
            };
            resets.push(stmt);
        }
        resets
    }

    /// Generate the `latency_samples()` method on the graph struct.
    fn generate_latency_method(&self) -> TokenStream {
        let down_latencies: Vec<_> = self
            .edges()
            .filter_map(|(idx, e)| match e.kernel {
                EdgeKernel::Down { factor, .. } => {
                    let f = resampler_field_name(idx);
                    let factor_lit = factor as usize;
                    let projected = self.cross_rate_kernel_state_type(e).is_some();
                    let access = if projected {
                        quote! { .kernel }
                    } else {
                        quote! {}
                    };
                    let one = if let FanoutShape::Parallel { .. } = e.fanout {
                        quote! {
                            total += ::oscen::resample::StreamDownsampler::latency_samples(&self.#f[0] #access) / #factor_lit;
                        }
                    } else {
                        quote! {
                            total += ::oscen::resample::StreamDownsampler::latency_samples(&self.#f #access) / #factor_lit;
                        }
                    };
                    Some(one)
                }
                _ => None,
            })
            .collect();

        quote! {
            /// Outer-rate latency in samples introduced by all multi-rate downsamplers.
            pub fn latency_samples(&self) -> usize {
                let mut total: usize = 0;
                #(#down_latencies)*
                total
            }
        }
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
    /// endpoint was lowered from a bare `ConnectionExpr::Ident` (i.e. a graph
    /// input accessed without a dot-field selector). In that case the endpoint
    /// name equals the node name and this returns `None` to preserve the
    /// pre-IR behaviour: `extract_endpoint_field(Ident(x)) == None`.
    ///
    /// For compound expressions descends into the left/receiver (leftmost-first)
    /// to find the first endpoint's field. Returns `None` for pure
    /// `Call`/`Literal`.
    ///
    /// This heuristic will be removed in T9 when `IrEdge` drops the AST fields
    /// and bare-input edges gain a dedicated IR representation.
    fn extract_endpoint_field<'e>(
        &'e self,
        expr: &'e crate::ir::expr::IrExpr,
    ) -> Option<&'e syn::Ident> {
        use crate::ir::expr::IrExprKind;
        match &expr.kind {
            IrExprKind::Endpoint(ep) => {
                // A bare `Ident` reference (graph input accessed without a field
                // selector) has endpoint == node.name. Treat that as "no field".
                let node_name = &self.ir.nodes[ep.node].name;
                if *node_name == ep.endpoint {
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
            IrExprKind::MethodCall { receiver, method, args } => {
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
    /// `self.voices[3].output`). For graph-level input/output nodes whose
    /// endpoint name matches the node name (bare-ident case), emits just
    /// `self.<name>` to match the legacy `ConnectionExpr::Ident` emission.
    fn emit_endpoint(&self, ep: &crate::ir::expr::IrEndpoint) -> TokenStream {
        let node_name = &self.ir.nodes[ep.node].name;
        let endpoint_name = &ep.endpoint;
        match ep.index {
            Some(idx) => quote! { self.#node_name[#idx].#endpoint_name },
            None => {
                // For bare-ident references (graph input/output nodes whose
                // endpoint name equals the node name), emit just self.<name>
                // — matches the legacy ConnectionExpr::Ident emission.
                if *node_name == *endpoint_name {
                    quote! { self.#node_name }
                } else {
                    quote! { self.#node_name.#endpoint_name }
                }
            }
        }
    }

    /// Same-rate Scalar → Scalar connection via `ConnectEndpoints`.
    fn emit_scalar_connect(
        &self,
        source_ident: &syn::Ident,
        source_access: &TokenStream,
        dest_node: &syn::Ident,
        dest_field: &syn::Ident,
    ) -> TokenStream {
        quote! {
            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                &self.#source_ident #source_access,
                &mut self.#dest_node.#dest_field
            );
        }
    }

    /// Same-rate Array → Array connection: parallel pairing, one
    /// `ConnectEndpoints` per index.
    fn emit_parallel_connect(
        &self,
        source_ident: &syn::Ident,
        source_access: &TokenStream,
        dest_node: &syn::Ident,
        dest_field: &syn::Ident,
        n: usize,
    ) -> TokenStream {
        quote! {
            for i in 0..#n {
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &self.#source_ident[i] #source_access,
                    &mut self.#dest_node[i].#dest_field
                );
            }
        }
    }

    /// Same-rate Scalar → Array broadcast.
    fn emit_broadcast_connect(
        &self,
        source_ident: &syn::Ident,
        source_access: &TokenStream,
        dest_node: &syn::Ident,
        dest_field: &syn::Ident,
        n: usize,
    ) -> TokenStream {
        quote! {
            for i in 0..#n {
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &self.#source_ident #source_access,
                    &mut self.#dest_node[i].#dest_field
                );
            }
        }
    }

    /// Same-rate Array → Scalar fan-in.
    fn emit_fanin_connect(
        &self,
        source_ident: &syn::Ident,
        source_field: Option<&syn::Ident>,
        dest_node: &syn::Ident,
        dest_field: &syn::Ident,
    ) -> TokenStream {
        if let Some(field) = source_field {
            quote! {
                self.#dest_node.#dest_field = self.#source_ident.iter().map(|n| n.#field).sum();
            }
        } else {
            quote! {
                self.#dest_node.#dest_field = self.#source_ident.iter().sum();
            }
        }
    }

    /// Generate connection assignments for a specific node
    fn generate_connection_assignments_for_node(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        self.generate_connection_assignments_for_node_filtered(node_name, |_| true)
    }

    /// Like `generate_connection_assignments_for_node` but only emits assignments
    /// for connections whose `EdgeKernel` matches `keep`.
    fn generate_connection_assignments_for_node_filtered<F>(
        &self,
        node_name: &syn::Ident,
        keep: F,
    ) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut assignments = Vec::new();

        for (_, edge) in self.edges() {
            if !keep(&edge.kernel) {
                continue;
            }
            let source = &edge.source;
            let dest = &edge.dest;
            let dest_node = &self.ir.nodes[dest.node].name;
            let dest_field = &dest.endpoint;

            if dest_node != node_name {
                continue;
            }

            // Compound sources (arithmetic, function/method calls) don't have
            // a single root endpoint. Evaluate them as f32 and route via
            // ConnectEndpoints<f32, _>.
            if !Self::is_simple_endpoint_source(source) {
                let src_tokens = self.emit_expr(source);
                if let Some(dest_size) = self.get_node_array_size(dest_node) {
                    assignments.push(quote! {
                        {
                            let __src: f32 = #src_tokens;
                            for i in 0..#dest_size {
                                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                    &__src,
                                    &mut self.#dest_node[i].#dest_field,
                                );
                            }
                        }
                    });
                } else {
                    assignments.push(quote! {
                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                            &(#src_tokens),
                            &mut self.#dest_node.#dest_field,
                        );
                    });
                }
                continue;
            }

            // This connection feeds into the current node
            if let Some(source_ident) = self.extract_root_node(source) {
                let source_field = self.extract_endpoint_field(source);

                // Check if source is a graph input (not a node)
                let source_is_graph_input = self.is_input(source_ident);

                // Skip voice array marker connections
                if let Some(field) = source_field {
                    if *field == "voices" {
                        if let Some(dest_array_size) = self.get_node_array_size(dest_node) {
                            assignments.push(quote! {
                                for i in 0..#dest_array_size {
                                    <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                        &self.#source_ident.voices[i],
                                        &mut self.#dest_node[i].#dest_field
                                    );
                                }
                            });
                        }
                        continue;
                    }
                }

                // Construct source expression part
                // For ramped graph inputs, we need to access .current to get the f32 value
                let source_access = if source_is_graph_input
                    && source_field.is_none()
                    && self.is_ramped_input(source_ident).is_some()
                {
                    quote! { .current }
                } else if let Some(field) = source_field {
                    quote! { .#field }
                } else {
                    quote! {}
                };

                let shape = edge.fanout;
                let stmt = match shape {
                    FanoutShape::Scalar => self.emit_scalar_connect(
                        source_ident,
                        &source_access,
                        dest_node,
                        dest_field,
                    ),
                    FanoutShape::Parallel { n } => self.emit_parallel_connect(
                        source_ident,
                        &source_access,
                        dest_node,
                        dest_field,
                        n,
                    ),
                    FanoutShape::Broadcast { n } => self.emit_broadcast_connect(
                        source_ident,
                        &source_access,
                        dest_node,
                        dest_field,
                        n,
                    ),
                    FanoutShape::FanIn { n: _ } => self.emit_fanin_connect(
                        source_ident,
                        source_field,
                        dest_node,
                        dest_field,
                    ),
                };
                assignments.push(stmt);
            }
        }

        assignments
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

    /// Emit `process_event_inputs()` + `process()` for a single node.
    fn emit_node_process_call(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process_event_inputs();
                    self.#node_name[i].process();
                }
            }
        } else {
            quote! {
                self.#node_name.process_event_inputs();
                self.#node_name.process();
            }
        }
    }

    /// Emit only `process()` for a single node.
    fn emit_node_process_only(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process();
                }
            }
        } else {
            quote! {
                self.#node_name.process();
            }
        }
    }

    /// Emit `process_event_inputs()` for a single node.
    fn emit_node_process_event_inputs(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process_event_inputs();
                }
            }
        } else {
            quote! {
                self.#node_name.process_event_inputs();
            }
        }
    }

    /// Emit assignments for connections that target graph outputs.
    fn generate_graph_output_assignments_filtered<F>(&self, keep: F) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut out = Vec::new();
        for (_, edge) in self.edges() {
            if !keep(&edge.kernel) {
                continue;
            }
            let source = &edge.source;
            let dest = &edge.dest;
            let dest_ident = &self.ir.nodes[dest.node].name;
            if let Some(output_kind) = self.output_kind(dest_ident) {
                let source_node = self.extract_root_node(source);
                let source_field = self.extract_endpoint_field(source);
                // Treat as "simple" only for a plain `node.field` endpoint
                // reference. Compound expressions (Binary, MethodCall) and bare
                // ident refs (graph inputs accessed without a dot-selector) are
                // not simple: the old code returned `None` for
                // `extract_endpoint_field(Ident(_))` so the `is_some() &&
                // is_some()` check was false for them too.
                let is_simple_source =
                    Self::is_simple_endpoint_source(source) && source_field.is_some();

                match output_kind {
                    EndpointKind::Stream | EndpointKind::Value => {
                        if is_simple_source {
                            let source_node = source_node.unwrap();
                            let source_field = source_field.unwrap();
                            if let Some(_src_array_size) = self.get_node_array_size(source_node) {
                                out.push(quote! {
                                    self.#dest_ident = self.#source_node.iter().map(|n| n.#source_field).sum();
                                });
                            } else {
                                out.push(quote! {
                                    <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                        &self.#source_node.#source_field,
                                        &mut self.#dest_ident
                                    );
                                });
                            }
                        } else {
                            let source_tokens = self.emit_expr(source);
                            out.push(quote! {
                                self.#dest_ident = #source_tokens;
                            });
                        }
                    }
                    EndpointKind::Event => {
                        if is_simple_source {
                            let source_node = source_node.unwrap();
                            let source_field = source_field.unwrap();
                            if let Some(array_size) = self.get_node_array_size(source_node) {
                                out.push(quote! {
                                    self.#dest_ident.clear();
                                    for i in 0..#array_size {
                                        for event in self.#source_node[i].#source_field.iter() {
                                            let _ = self.#dest_ident.try_push(event.clone());
                                        }
                                    }
                                });
                            } else {
                                out.push(quote! {
                                    <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                        &self.#source_node.#source_field,
                                        &mut self.#dest_ident
                                    );
                                });
                            }
                        }
                    }
                }
            }
        }
        out
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

    // ========== Block Processing Methods ==========

    /// Generate the `__advance_one_frame()` private method.
    fn generate_advance_one_frame(&self) -> Result<TokenStream> {
        if self.max_factor() <= 1 {
            self.generate_advance_one_frame_same_rate()
        } else {
            self.generate_advance_one_frame_multirate()
        }
    }

    /// Same-rate fast path.
    fn generate_advance_one_frame_same_rate(&self) -> Result<TokenStream> {
        let process_body = self.generate_process_body()?;

        // Read stream inputs from block buffers
        let stream_input_reads: Vec<_> = self
            .inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Stream)))
            .map(|n| {
                let name = &n.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#name = self.#block_name[__frame]; }
            })
            .collect();

        // Write stream outputs to block buffers
        let stream_output_writes: Vec<_> = self
            .outputs()
            .filter(|n| matches!(self.output_kind(&n.name), Some(EndpointKind::Stream)))
            .map(|n| {
                let name = &n.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#block_name[__frame] = self.#name; }
            })
            .collect();

        Ok(quote! {
            #[inline(always)]
            #[allow(unused_variables)]
            fn __advance_one_frame(&mut self, __frame: usize) {
                use ::oscen::SignalProcessor as _;

                #(#stream_input_reads)*

                self.tick_ramps();

                #(#process_body)*

                #(#stream_output_writes)*
            }
        })
    }

    /// Multi-rate variant of `__advance_one_frame`.
    fn generate_advance_one_frame_multirate(&self) -> Result<TokenStream> {
        let body = self.generate_multirate_inner_body()?;

        let stream_input_reads: Vec<_> = self
            .inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Stream)))
            .map(|n| {
                let name = &n.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#name = self.#block_name[__frame]; }
            })
            .collect();

        let stream_output_writes: Vec<_> = self
            .outputs()
            .filter(|n| matches!(self.output_kind(&n.name), Some(EndpointKind::Stream)))
            .map(|n| {
                let name = &n.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#block_name[__frame] = self.#name; }
            })
            .collect();

        Ok(quote! {
            #[inline(always)]
            #[allow(unused_variables, unused_mut)]
            fn __advance_one_frame(&mut self, __frame: usize) {
                // 1. Read stream inputs from block buffers (outer-rate).
                #(#stream_input_reads)*

                // 2-8. Multi-rate body
                #body

                // 9. Write stream outputs to block buffers (outer-rate).
                #(#stream_output_writes)*
            }
        })
    }

    /// Emit the multi-rate body.
    fn generate_multirate_inner_body(&self) -> Result<TokenStream> {
        let max_factor = self.max_factor() as usize;
        let sorted_nodes: Vec<syn::Ident> = self.nodes().map(|n| n.name.clone()).collect();

        // Bucket nodes by rate.
        let outer_node_names: Vec<syn::Ident> = sorted_nodes
            .iter()
            .filter(|name| matches!(self.node_rate(name), NodeRate::Same))
            .cloned()
            .collect();
        let inner_node_names: Vec<syn::Ident> = sorted_nodes
            .iter()
            .filter(|name| matches!(self.node_rate(name), NodeRate::Up(_)))
            .cloned()
            .collect();

        let tainted = self.compute_post_inner_same_nodes()?;
        let pre_inner_outer_names: Vec<syn::Ident> = outer_node_names
            .iter()
            .filter(|n| !tainted.contains(&n.to_string()))
            .cloned()
            .collect();
        let post_inner_outer_names: Vec<syn::Ident> = outer_node_names
            .iter()
            .filter(|n| tainted.contains(&n.to_string()))
            .cloned()
            .collect();

        // Step 3: Outer-rate node processing — pre-inner bucket only.
        let mut outer_process: Vec<TokenStream> = Vec::new();
        for node_name in &pre_inner_outer_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            outer_process.extend(assignments);
            outer_process.push(self.emit_node_process_call(node_name));
        }

        // Step 7.5: Post-inner outer-rate node processing.
        let mut post_inner_outer_process: Vec<TokenStream> = Vec::new();
        for node_name in &post_inner_outer_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            post_inner_outer_process.extend(assignments);
            post_inner_outer_process.push(self.emit_node_process_call(node_name));
        }

        // Step 4: Per-edge upsample warmup for `EdgeKernel::Up` connections.
        let mut up_decls: Vec<TokenStream> = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Up { factor, .. } = edge.kernel {
                let factor_us = factor as usize;
                let buf = up_buf_name(idx);
                let field = resampler_field_name(idx);
                let projected = self.cross_rate_kernel_state_type(edge).is_some();
                let access = if projected {
                    quote! { .kernel }
                } else {
                    quote! {}
                };

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let source_ident = self.extract_root_node(&edge.source)
                        .expect("Parallel edge has array root");
                    let source_field = self.extract_endpoint_field(&edge.source)
                        .expect("Parallel edge has field access");
                    up_decls.push(quote! {
                        let mut #buf: [[f32; #factor_us]; #n] = [[0.0; #factor_us]; #n];
                        for __k in 0..#n {
                            let mut __src_val: f32 = 0.0;
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &self.#source_ident[__k].#source_field,
                                &mut __src_val,
                            );
                            ::oscen::resample::StreamUpsampler::upsample(
                                &mut self.#field[__k] #access,
                                __src_val,
                                &mut #buf[__k],
                            );
                        }
                    });
                } else {
                    let src_value =
                        self.connection_source_value_expr(&edge.source);
                    up_decls.push(quote! {
                        let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
                        {
                            let __src_val: f32 = #src_value;
                            ::oscen::resample::StreamUpsampler::upsample(
                                &mut self.#field #access,
                                __src_val,
                                &mut #buf,
                            );
                        }
                    });
                }
            }
        }

        // Step 5: Per-edge accumulator buffers for `EdgeKernel::Down` connections.
        let mut down_decls: Vec<TokenStream> = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { factor, .. } = edge.kernel {
                let factor_us = factor as usize;
                let buf = down_buf_name(idx);
                if let FanoutShape::Parallel { n } = edge.fanout {
                    down_decls.push(quote! {
                        let mut #buf: [[f32; #factor_us]; #n] = [[0.0; #factor_us]; #n];
                    });
                } else {
                    down_decls.push(quote! {
                        let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
                    });
                }
            }
        }

        // Outer -> inner cross-rate event drains.
        let mut event_outer_to_inner_drains: Vec<TokenStream> = Vec::new();
        for (_, edge) in self.edges() {
            if let EdgeKernel::Event {
                rescale: EventRescale::Multiply(n),
            } = edge.kernel
            {
                let drain = self.generate_event_drain(
                    &edge.source,
                    &edge.dest,
                    EventRescale::Multiply(n),
                );
                event_outer_to_inner_drains.push(drain);
            }
        }

        // Inner -> outer cross-rate event drains.
        let mut event_inner_to_outer_drains: Vec<TokenStream> = Vec::new();
        for (_, edge) in self.edges() {
            if let EdgeKernel::Event {
                rescale: EventRescale::Divide(n),
            } = edge.kernel
            {
                let drain = self.generate_event_drain(
                    &edge.source,
                    &edge.dest,
                    EventRescale::Divide(n),
                );
                event_inner_to_outer_drains.push(drain);
            }
        }

        // Run process_event_inputs() for inner-rate nodes once per outer tick.
        let inner_event_input_calls: Vec<TokenStream> = inner_node_names
            .iter()
            .map(|n| self.emit_node_process_event_inputs(n))
            .collect();

        let mut inner_writes: Vec<TokenStream> = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Up { .. } = edge.kernel {
                let buf = up_buf_name(idx);

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let dest_node = &self.ir.nodes[edge.dest.node].name;
                    let dest_field = &edge.dest.endpoint;
                    inner_writes.push(quote! {
                        for __k in 0..#n {
                            let __dst_val: f32 = #buf[__k][__inner];
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &__dst_val,
                                &mut self.#dest_node[__k].#dest_field,
                            );
                        }
                    });
                } else {
                    let dest_assign = self.connection_dest_field_assign(
                        &edge.dest,
                        &quote! { #buf[__inner] },
                    );
                    inner_writes.push(dest_assign);
                }
            }
        }

        let mut inner_node_runs: Vec<TokenStream> = Vec::new();
        for node_name in &inner_node_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            inner_node_runs.extend(assignments);
            inner_node_runs.push(self.emit_node_process_only(node_name));
        }

        let mut down_captures: Vec<TokenStream> = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { .. } = edge.kernel {
                let buf = down_buf_name(idx);

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let source_ident = self.extract_root_node(&edge.source)
                        .expect("Parallel edge has array root");
                    let source_field = self.extract_endpoint_field(&edge.source)
                        .expect("Parallel edge has field access");
                    down_captures.push(quote! {
                        for __k in 0..#n {
                            let mut __elt: f32 = 0.0;
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &self.#source_ident[__k].#source_field,
                                &mut __elt,
                            );
                            #buf[__k][__inner] = __elt;
                        }
                    });
                } else {
                    let src_value =
                        self.connection_source_value_expr(&edge.source);
                    down_captures.push(quote! {
                        #buf[__inner] = #src_value;
                    });
                }
            }
        }

        // Step 7: Finalize Down edges.
        let mut down_finalizes: Vec<TokenStream> = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { .. } = edge.kernel {
                let buf = down_buf_name(idx);
                let field = resampler_field_name(idx);
                let projected = self.cross_rate_kernel_state_type(edge).is_some();
                let access = if projected {
                    quote! { .kernel }
                } else {
                    quote! {}
                };

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let dest_node = &self.ir.nodes[edge.dest.node].name;
                    let dest_field = &edge.dest.endpoint;
                    down_finalizes.push(quote! {
                        for __k in 0..#n {
                            let __dst_val: f32 = ::oscen::resample::StreamDownsampler::downsample(
                                &mut self.#field[__k] #access,
                                &#buf[__k],
                            );
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &__dst_val,
                                &mut self.#dest_node[__k].#dest_field,
                            );
                        }
                    });
                } else {
                    let dest_assign = self.connection_dest_field_assign(
                        &edge.dest,
                        &quote! {
                            ::oscen::resample::StreamDownsampler::downsample(
                                &mut self.#field #access,
                                &#buf,
                            )
                        },
                    );
                    down_finalizes.push(dest_assign);
                }
            }
        }

        // Step 8: Same-rate connection assignments to graph outputs.
        let same_rate_output_trailer =
            self.generate_graph_output_assignments_filtered(is_same_rate_kernel);

        Ok(quote! {
            {
                use ::oscen::SignalProcessor as _;

                // 2. Tick ramped value inputs at outer rate.
                self.tick_ramps();

                // 3. Outer-rate (Same) nodes process once per outer tick.
                #(#outer_process)*

                // 4. Per-edge upsample warmup for cross-rate Up edges.
                #(#up_decls)*

                // 5. Per-edge accumulator buffers for cross-rate Down edges.
                #(#down_decls)*

                // 5.5. Cross-rate event drains: outer -> inner.
                #(#event_outer_to_inner_drains)*

                // 6a. Run process_event_inputs() once per outer tick for inner nodes.
                #(#inner_event_input_calls)*

                // 6. Inner loop: ×N nodes run N times per outer tick.
                for __inner in 0..#max_factor {
                    #(#inner_writes)*
                    #(#inner_node_runs)*
                    #(#down_captures)*
                }

                // 7. Downsample once per outer tick into dest fields.
                #(#down_finalizes)*

                // 7a. Cross-rate event drains: inner -> outer.
                #(#event_inner_to_outer_drains)*

                // 7.5. Post-inner outer-rate nodes.
                #(#post_inner_outer_process)*

                // 8. Same-rate trailer assignments (e.g., to graph outputs).
                #(#same_rate_output_trailer)*
            }
        })
    }

    /// Emit the cross-rate event drain for one edge.
    fn generate_event_drain(
        &self,
        source: &crate::ir::expr::IrExpr,
        dest: &crate::ir::expr::IrEndpoint,
        rescale: EventRescale,
    ) -> TokenStream {
        let transform = match rescale {
            EventRescale::Multiply(n) => {
                quote! { __ev.frame_offset = __ev.frame_offset.saturating_mul(#n); }
            }
            EventRescale::Divide(n) => {
                quote! { __ev.frame_offset /= #n; }
            }
            EventRescale::None => quote! {},
        };

        let src_size = match self.extract_root_node(source) {
            Some(ident) => self.get_node_array_size(ident),
            None => None,
        };
        let dest_node_name = &self.ir.nodes[dest.node].name;
        let dst_size = self.get_node_array_size(dest_node_name);
        // src_field_access: source is a plain `node.field` endpoint (not bare-ident, not compound).
        let src_field_access =
            Self::is_simple_endpoint_source(source)
                && self.extract_endpoint_field(source).is_some();
        // dst_field_access: dest has a separate endpoint field (not bare-ident graph output).
        let dst_field_access = *dest_node_name != dest.endpoint;

        // Broadcast: scalar source → array dest field.
        if let (None, Some(n), true) = (src_size, dst_size, dst_field_access) {
            let dest_node = dest_node_name;
            let dest_field = &dest.endpoint;
            let source_tokens = self.emit_expr(source);
            return quote! {
                {
                    for __k in 0..#n {
                        self.#dest_node[__k].#dest_field.clear();
                    }
                    for __ev_ref in #source_tokens.iter() {
                        let mut __ev = __ev_ref.clone();
                        #transform
                        for __k in 0..#n {
                            let _ = self.#dest_node[__k].#dest_field.try_push(__ev.clone());
                        }
                    }
                }
            };
        }

        // FanIn: array source field → scalar dest.
        if let (Some(n), None, true) = (src_size, dst_size, src_field_access) {
            let source_ident = self.extract_root_node(source).expect("checked");
            let source_field = self.extract_endpoint_field(source).expect("Field has field");
            let dest_tokens = self.emit_endpoint(dest);
            return quote! {
                {
                    #dest_tokens.clear();
                    for __k in 0..#n {
                        for __ev_ref in self.#source_ident[__k].#source_field.iter() {
                            let mut __ev = __ev_ref.clone();
                            #transform
                            let _ = #dest_tokens.try_push(__ev);
                        }
                    }
                }
            };
        }

        // Parallel: array source field → array dest field.
        if let (Some(ns), Some(nd), true, true) =
            (src_size, dst_size, src_field_access, dst_field_access)
        {
            let n = ns.min(nd);
            let source_ident = self.extract_root_node(source).expect("checked");
            let source_field = self.extract_endpoint_field(source).expect("Field has field");
            let dest_node = dest_node_name;
            let dest_field = &dest.endpoint;
            return quote! {
                {
                    for __k in 0..#n {
                        self.#dest_node[__k].#dest_field.clear();
                        for __ev_ref in self.#source_ident[__k].#source_field.iter() {
                            let mut __ev = __ev_ref.clone();
                            #transform
                            let _ = self.#dest_node[__k].#dest_field.try_push(__ev);
                        }
                    }
                }
            };
        }

        // Scalar (or indexed access on either side): single drain.
        let source_tokens = self.emit_expr(source);
        let dest_tokens = self.emit_endpoint(dest);
        quote! {
            {
                #dest_tokens.clear();
                for __ev_ref in #source_tokens.iter() {
                    let mut __ev = __ev_ref.clone();
                    #transform
                    let _ = #dest_tokens.try_push(__ev);
                }
            }
        }
    }

    /// Compute the closure of `Same` nodes that must run AFTER the multi-rate
    /// inner loop because they consume a `Down` edge.
    fn compute_post_inner_same_nodes(&self) -> Result<HashSet<String>> {
        let mut tainted: HashSet<String> = HashSet::new();
        let same_rate = |name: &str| {
            self.find_node_by_name(name)
                .map(|n| matches!(n.rate, NodeRate::Same))
                .unwrap_or(true) // graph endpoints behave as same-rate
        };

        for (_, edge) in self.edges() {
            // Outer-rate consumers of any inner-produced data must run after
            // the inner loop. This includes both downsampled stream/value
            // edges and inner -> outer event drains.
            let is_inner_produced = matches!(
                edge.kernel,
                EdgeKernel::Down { .. }
                    | EdgeKernel::Event {
                        rescale: EventRescale::Divide(_)
                    }
            );
            if is_inner_produced {
                let dst_name = self.ir.nodes[edge.dest.node].name.to_string();
                if same_rate(&dst_name) {
                    tainted.insert(dst_name);
                }
            }
        }

        // Propagate through Same-rate edges until fixpoint.
        let mut changed = true;
        while changed {
            changed = false;
            for (_, edge) in self.edges() {
                if !matches!(edge.kernel, EdgeKernel::None) {
                    continue;
                }
                let (Some(src), Some(dst)) = (
                    root_node_name(&edge.source, self.ir),
                    Some(self.ir.nodes[edge.dest.node].name.to_string()),
                ) else {
                    continue;
                };
                if !same_rate(&dst) {
                    continue;
                }
                if tainted.contains(&src) && !tainted.contains(&dst) {
                    tainted.insert(dst);
                    changed = true;
                }
            }
        }

        // Diamond detection.
        for (_, edge) in self.edges() {
            if let EdgeKernel::Up { .. } = edge.kernel {
                if let Some(src) = root_node_name(&edge.source, self.ir) {
                    if tainted.contains(&src) {
                        return Err(syn::Error::new(
                            edge.span,
                            "v1 limitation: a same-rate node downstream of a downsampled (cross-rate) edge cannot itself feed an oversampled (`* N`) node — \
                             the single-pass multi-rate pipeline can't service two cross-rate boundaries chained through a same-rate intermediate. \
                             Route the oversampled side directly from the original source instead.",
                        ));
                    }
                }
            }
        }

        Ok(tainted)
    }

    /// Build an `f32`-valued expression for a connection's source.
    fn connection_source_value_expr(
        &self,
        source: &crate::ir::expr::IrExpr,
    ) -> TokenStream {
        // Compound or non-trivial sources.
        if !Self::is_simple_endpoint_source(source) {
            let toks = self.emit_expr(source);
            return quote! { (#toks) as f32 };
        }

        // FanIn sum: source is `<array_node>.<field>` with no explicit index.
        let src_array_size = match self.extract_root_node(source) {
            Some(ident) => self.get_node_array_size(ident),
            None => None,
        };
        // source_is_field_access: simple endpoint where endpoint != node_name (not bare-ident).
        let source_is_field_access =
            Self::is_simple_endpoint_source(source)
                && self.extract_endpoint_field(source).is_some();

        if let (Some(n), true) = (src_array_size, source_is_field_access) {
            let source_ident = self.extract_root_node(source).expect("checked above");
            let source_field =
                self.extract_endpoint_field(source).expect("Field variant has a field");
            return quote! {
                {
                    let mut __sum: f32 = 0.0;
                    for i in 0..#n {
                        let mut __elt: f32 = 0.0;
                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                            &self.#source_ident[i].#source_field,
                            &mut __elt,
                        );
                        __sum += __elt;
                    }
                    __sum
                }
            };
        }

        // Ramped graph value input: read .current directly.
        // In IR, a bare-ident reference has endpoint == node_name.
        if let crate::ir::expr::IrExprKind::Endpoint(ep) = &source.kind {
            let node_name = &self.ir.nodes[ep.node].name;
            if *node_name == ep.endpoint
                && self.is_input(node_name)
                && self.is_ramped_input(node_name).is_some()
            {
                return quote! { self.#node_name.current };
            }
        }

        // Simple scalar endpoint source.
        let toks = self.emit_expr(source);
        quote! {
            {
                let mut __src: f32 = 0.0;
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &#toks,
                    &mut __src,
                );
                __src
            }
        }
    }

    /// Build an assignment `dest <- value` for a connection's dest.
    fn connection_dest_field_assign(
        &self,
        dest: &crate::ir::expr::IrEndpoint,
        value: &TokenStream,
    ) -> TokenStream {
        let dest_node = &self.ir.nodes[dest.node].name;
        let dest_field = &dest.endpoint;

        // Graph output (bare-ident, no separate field): direct assignment.
        // In IR, bare-ident has endpoint == node_name.
        if self.is_output(dest_node) && *dest_node == *dest_field {
            return quote! { self.#dest_node = #value; };
        }

        let dest_array_size = self.get_node_array_size(dest_node);
        // dest_is_field_access: non-bare-ident endpoint on a scalar node.
        let dest_is_field_access = *dest_node != *dest_field && dest.index.is_none();

        if let (Some(n), true) = (dest_array_size, dest_is_field_access) {
            // Broadcast write: dest is `<array_node>.<field>`.
            return quote! {
                {
                    let __dst_val: f32 = #value;
                    for i in 0..#n {
                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                            &__dst_val,
                            &mut self.#dest_node[i].#dest_field,
                        );
                    }
                }
            };
        }

        // Scalar dest (or indexed array element): single ConnectEndpoints write.
        let dest_toks = self.emit_endpoint(dest);
        quote! {
            {
                let __dst_val: f32 = #value;
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &__dst_val,
                    &mut #dest_toks,
                );
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

        let node_init_calls = self.generate_node_init_calls_rate_aware();
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
                fn init(&mut self, sample_rate: f32) {
                    self.sample_rate = sample_rate;
                    // Call init() on all child nodes, scaling sample_rate by
                    // each node's rate annotation.
                    #(#node_init_calls)*
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
