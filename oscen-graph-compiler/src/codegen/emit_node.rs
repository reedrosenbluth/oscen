//! Per-node emitters: outer/inner process calls, event input dispatch,
//! taint analysis, and per-node incoming-edge assignment.

use crate::ast::{EndpointKind, NodeRate};
use crate::ir::expr::IrEndpoint;
use crate::ir::graph::{EdgeKernel, EventRescale, FanoutShape, IrEdge, NodeId};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use std::collections::HashSet;
use syn::Result;

use super::helpers::root_node_name;
use super::CodegenContext;

/// How a stream destination's incoming edges should be emitted, after
/// classifying them for auto-summing fan-in.
enum StreamFanin<'e> {
    /// Fewer than two incoming stream edges (or a non-stream endpoint): emit
    /// per-edge exactly as before. Single-source graphs stay byte-identical.
    Single,
    /// Two or more same-rate simple scalar/frame stream sources: emit one
    /// summed assignment (`dest = src1 + src2 + …`), edges in canonical order.
    Sum(Vec<&'e IrEdge>),
    /// Two or more incoming edges where at least one is not a same-rate simple
    /// scalar source: emit a scoped `compile_error!` rather than wrong audio.
    Unsupported { span: Span, message: String },
}

impl<'a> CodegenContext<'a> {
    /// Classify the edges feeding a stream destination slot for auto-summing
    /// fan-in. Only stream endpoints with ≥2 incoming edges are bucketed; the
    /// decision is computed over the destination's *full* incoming-edge set (in
    /// canonical `edge_order`), so the same-rate/simple/scalar guard holds no
    /// matter which `keep` pass the caller is in.
    fn classify_stream_fanin(&self, dest: &IrEndpoint) -> StreamFanin<'_> {
        // Endpoints with a *known* value/event/asset kind are out of scope and
        // stay on the per-edge path. Kinds are only seeded from a typed graph
        // endpoint or a stream-resampling policy (see `lower::endpoint_kind_of`),
        // so pure node-to-node endpoints stay `None` (unknown) and are treated
        // as stream-summable below. Consequences for the unknown case:
        //   * stream f32/Frame fan-in sums (the feature);
        //   * event fan-in still works — `AccumulateEndpoints` delegates events
        //     to `connect` (last-write-wins, unchanged), so it compiles;
        //   * an unknown-kind *value* f32 fan-in is summed rather than
        //     last-write-wins. There is no kind info to tell it apart from a
        //     stream f32 fan-in; summing matches Cmajor's rule and is the same
        //     tradeoff that makes the node-to-node stream case work.
        let kind = self.ir.nodes[dest.node]
            .endpoints
            .get(&dest.endpoint)
            .map(|e| e.kind);
        if matches!(
            kind,
            Some(EndpointKind::Value | EndpointKind::Event | EndpointKind::Asset)
        ) {
            return StreamFanin::Single;
        }

        // Every edge feeding this exact destination slot, canonical order.
        let bucket: Vec<&IrEdge> = self
            .ir
            .edge_order
            .iter()
            .map(|&eid| &self.ir.edges[eid])
            .filter(|e| {
                e.dest.node == dest.node
                    && e.dest.endpoint == dest.endpoint
                    && e.dest.index == dest.index
            })
            .collect();
        if bucket.len() < 2 {
            return StreamFanin::Single;
        }

        // Same-rate event fan-in (`EdgeKernel::Event`) accumulates via the
        // existing try_push path — never sum it. Leave the whole bucket on the
        // per-edge path unchanged.
        if bucket
            .iter()
            .any(|e| matches!(e.kernel, EdgeKernel::Event { .. }))
        {
            return StreamFanin::Single;
        }

        // ≥2 sources: a plain `+` sum is only well-typed for same-rate, simple,
        // scalar sources. Anything else is rejected with a scoped message.
        for e in &bucket {
            let disqualifier = if !matches!(e.kernel, EdgeKernel::None) {
                Some("a cross-rate edge")
            } else if !Self::is_simple_endpoint_source(&e.source)
                || self.extract_root_node(&e.source).is_none()
            {
                Some("a compound (non-endpoint) source")
            } else {
                match e.fanout {
                    FanoutShape::Scalar => None,
                    FanoutShape::Parallel { .. } => Some("an array (parallel) source"),
                    FanoutShape::Broadcast { .. } => Some("a broadcast source"),
                    FanoutShape::FanIn { .. } => Some("an array fan-in source"),
                }
            };
            if let Some(what) = disqualifier {
                return StreamFanin::Unsupported {
                    span: e.span,
                    message: self.fanin_unsupported_message(dest, what),
                };
            }
        }

        StreamFanin::Sum(bucket)
    }

    /// Message for an unsupported multi-source stream destination.
    fn fanin_unsupported_message(&self, dest: &IrEndpoint, what: &str) -> String {
        let dest_desc = if dest.bare {
            self.ir.nodes[dest.node].name.to_string()
        } else {
            format!("{}.{}", self.ir.nodes[dest.node].name, dest.endpoint)
        };
        format!(
            "fan-in summing supports only same-rate scalar/frame stream sources; \
             saw {what} into `{dest_desc}`"
        )
    }

    /// The source-access expression a scalar connect would read, e.g.
    /// `self.osc.output` (node endpoint) or `self.dry` (graph input). Used as a
    /// single term of a fan-in sum. The source is known simple + scalar.
    fn simple_source_access_tokens(&self, source: &crate::ir::expr::IrExpr) -> TokenStream {
        let source_ident = self
            .extract_root_node(source)
            .expect("simple scalar source has a root node");
        let source_field = self.extract_endpoint_field(source);
        let source_access = if self.is_input(source_ident)
            && source_field.is_none()
            && self.is_ramped_input(source_ident).is_some()
        {
            quote! { .current }
        } else if let Some(field) = source_field {
            quote! { .#field }
        } else {
            quote! {}
        };
        quote! { self.#source_ident #source_access }
    }

    /// Emit a fan-in sum into `dst` (the destination lvalue `self.node.field`
    /// or `self.out`): one `ConnectEndpoints::connect` for the first source,
    /// then one `AccumulateEndpoints::accumulate` per remaining source. For
    /// stream payloads (`f32`/`Frame<N>`) this sums element-wise; for event
    /// endpoints `accumulate` delegates to `connect`, preserving the existing
    /// last-write-wins behavior (and compiling — event queues have no `Add`).
    fn emit_stream_sum_assign(&self, sources: &[&IrEdge], dst: &TokenStream) -> TokenStream {
        let terms: Vec<TokenStream> = sources
            .iter()
            .map(|e| self.simple_source_access_tokens(&e.source))
            .collect();
        let (first, rest) = terms.split_first().expect("fan-in bucket has ≥2 sources");
        let accumulations = rest.iter().map(|term| {
            quote! {
                <() as ::oscen::graph::AccumulateEndpoints<_, _>>::accumulate(
                    &#term,
                    &mut #dst,
                );
            }
        });
        quote! {
            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                &#first,
                &mut #dst,
            );
            #(#accumulations)*
        }
    }

    /// Stable key for a destination slot, to emit each fan-in bucket once.
    fn fanin_bucket_key(dest: &IrEndpoint) -> (NodeId, String, Option<usize>) {
        (dest.node, dest.endpoint.to_string(), dest.index)
    }

    /// True when a source expression is a `Frame(...)` constructor call (a
    /// frame-valued connection source, e.g. `Frame::<2>(a, b)`).
    fn is_frame_constructor_source(source: &crate::ir::expr::IrExpr) -> bool {
        matches!(
            &source.kind,
            crate::ir::expr::IrExprKind::Call { function, .. } if function == "Frame"
        )
    }

    /// Generate connection assignments for a specific node.
    pub(super) fn generate_connection_assignments_for_node(
        &self,
        node_name: &syn::Ident,
    ) -> Vec<TokenStream> {
        self.generate_connection_assignments_for_node_filtered(node_name, |_| true)
    }

    /// Like `generate_connection_assignments_for_node` but only emits assignments
    /// for connections whose `EdgeKernel` matches `keep`.
    pub(super) fn generate_connection_assignments_for_node_filtered<F>(
        &self,
        node_name: &syn::Ident,
        keep: F,
    ) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut assignments = Vec::new();
        let mut emitted_fanin: HashSet<(NodeId, String, Option<usize>)> = HashSet::new();

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

            // Auto-summing fan-in: ≥2 same-rate simple scalar/frame stream
            // sources into one stream input become a single summed assignment
            // (or a scoped compile_error for unsupported multi-source shapes).
            // A single source falls through to the byte-identical per-edge path.
            match self.classify_stream_fanin(dest) {
                StreamFanin::Single => {}
                StreamFanin::Sum(sources) => {
                    if emitted_fanin.insert(Self::fanin_bucket_key(dest)) {
                        let target = quote! { self.#dest_node.#dest_field };
                        assignments.push(self.emit_stream_sum_assign(&sources, &target));
                    }
                    continue;
                }
                StreamFanin::Unsupported { span, message } => {
                    if emitted_fanin.insert(Self::fanin_bucket_key(dest)) {
                        assignments.push(quote_spanned! { span =>
                            ::core::compile_error!(#message);
                        });
                    }
                    continue;
                }
            }

            // Compound sources (arithmetic, function/method calls) don't have
            // a single root endpoint. Evaluate them as f32 and route via
            // ConnectEndpoints<f32, _>.
            if !Self::is_simple_endpoint_source(source) {
                let src_tokens = self.emit_expr(source);
                if let Some(dest_size) = self.get_node_array_size(dest_node) {
                    // The array-broadcast path binds `let __src: f32` (pinning
                    // numeric-literal inference to f32 where no single dest type
                    // is available). A frame constructor cannot flow through it —
                    // reject it with a scoped message rather than a confusing
                    // `expected f32, found Frame<_>` type error.
                    if Self::is_frame_constructor_source(source) {
                        assignments.push(quote_spanned! { source.span =>
                            ::core::compile_error!(
                                "a frame constructor cannot broadcast into a node array; \
                                 frame connection expressions are supported into scalar \
                                 stream destinations only"
                            );
                        });
                        continue;
                    }
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

                // A channel index on a scalar node's endpoint (`s.output[0]`)
                // extracts one channel of its `Frame<N>` value. (An index on a
                // node-array element is handled via the `[i]` node position and
                // keeps its existing access form.)
                let channel_index = Self::ir_expr_as_endpoint(source)
                    .and_then(|ep| ep.index)
                    .filter(|_| self.get_node_array_size(source_ident).is_none());

                // Construct source expression part
                // For ramped graph inputs, we need to access .current to get the f32 value
                let source_access = if source_is_graph_input
                    && source_field.is_none()
                    && self.is_ramped_input(source_ident).is_some()
                {
                    quote! { .current }
                } else if let Some(field) = source_field {
                    match channel_index {
                        Some(i) => quote! { .#field.0[#i] },
                        None => quote! { .#field },
                    }
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
                    FanoutShape::FanIn { n: _ } => {
                        self.emit_fanin_connect(source_ident, source_field, dest_node, dest_field)
                    }
                };
                assignments.push(stmt);
            }
        }

        assignments
    }

    /// Emit `process_event_inputs()` + `process()` for a single node.
    pub(super) fn emit_node_process_call(&self, node_name: &syn::Ident) -> TokenStream {
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
    pub(super) fn emit_node_process_only(&self, node_name: &syn::Ident) -> TokenStream {
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
    pub(super) fn emit_node_process_event_inputs(&self, node_name: &syn::Ident) -> TokenStream {
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
    pub(super) fn generate_graph_output_assignments_filtered<F>(&self, keep: F) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut out = Vec::new();
        let mut emitted_fanin: HashSet<(NodeId, String, Option<usize>)> = HashSet::new();
        for (_, edge) in self.edges() {
            if !keep(&edge.kernel) {
                continue;
            }
            let source = &edge.source;
            let dest = &edge.dest;
            let dest_ident = &self.ir.nodes[dest.node].name;
            if let Some(output_kind) = self.output_kind(dest_ident) {
                // Auto-summing fan-in for a multi-source top-level stream
                // output (single-source stays the byte-identical per-edge path).
                match self.classify_stream_fanin(dest) {
                    StreamFanin::Single => {}
                    StreamFanin::Sum(sources) => {
                        if emitted_fanin.insert(Self::fanin_bucket_key(dest)) {
                            let target = quote! { self.#dest_ident };
                            out.push(self.emit_stream_sum_assign(&sources, &target));
                        }
                        continue;
                    }
                    StreamFanin::Unsupported { span, message } => {
                        if emitted_fanin.insert(Self::fanin_bucket_key(dest)) {
                            out.push(quote_spanned! { span =>
                                ::core::compile_error!(#message);
                            });
                        }
                        continue;
                    }
                }

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
                    // Asset endpoints are bound from externals, never driven as
                    // a graph output connection.
                    EndpointKind::Asset => {}
                }
            }
        }
        out
    }

    /// Compute the closure of `Same` nodes that must run AFTER the multi-rate
    /// inner loop because they consume a `Down` edge.
    pub(super) fn compute_post_inner_same_nodes(&self) -> Result<HashSet<String>> {
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
}
