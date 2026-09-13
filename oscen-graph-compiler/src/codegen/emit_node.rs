//! Per-node emitters: outer/inner process calls, event input dispatch,
//! taint analysis, and per-node incoming-edge assignment.
//!
//! Connection assignments are emitted from the resolved [`DriverPlan`]:
//! one [`DriverGroup`] per destination slot, whose policy (single driver,
//! sum, append) was fixed during lowering. Nothing here re-derives fan-in
//! semantics from endpoint syntax.

use crate::ast::{EndpointKind, NodeRate};
use crate::ir::drivers::{Address, DriverGroup, Element};
use crate::ir::graph::{EdgeKernel, EventRescale, FanoutShape, IrEdge};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::Result;

use super::helpers::is_same_rate_kernel;
use super::CodegenContext;

impl<'a> CodegenContext<'a> {
    /// The lvalue tokens for a resolved address that names exactly one slot:
    /// `self.out` (bare graph endpoint), `self.node.field`, or
    /// `self.node[k].field`. Broadcast (`All`) addresses name several slots
    /// and are looped by their callers.
    fn emit_address(&self, a: &Address) -> TokenStream {
        let node = &self.ir.nodes[a.node].name;
        let field = &a.field;
        match a.element {
            Element::Index(k) => quote! { self.#node[#k].#field },
            Element::All { .. } => {
                unreachable!("broadcast address has no single lvalue; loop over elements")
            }
            Element::Scalar => {
                if a.bare {
                    quote! { self.#node }
                } else {
                    quote! { self.#node.#field }
                }
            }
        }
    }

    /// Emit an accumulating group (`Sum` / `Append`, or a single driver
    /// joining a broadcast): `connect` the first source into the slot, then
    /// `accumulate` every remaining one. For stream payloads this sums; for
    /// event endpoints `accumulate` appends, merging every source. A group
    /// joining a broadcast accumulates all of its sources, since the
    /// broadcast already initialized the slot.
    fn emit_accumulating_group(&self, g: &DriverGroup) -> TokenStream {
        let terms: Vec<TokenStream> = g
            .sources
            .iter()
            .map(|&eid| self.emit_expr(&self.ir.edges[eid].source))
            .collect();
        let dst = self.emit_address(&g.dest);
        let (first, rest) = terms.split_first().expect("accumulating group has a source");
        let lead = if g.onto_broadcast {
            quote! {
                <() as ::oscen::graph::AccumulateEndpoints<_, _>>::accumulate(
                    &#first,
                    &mut #dst,
                );
            }
        } else {
            quote! {
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &#first,
                    &mut #dst,
                );
            }
        };
        let accumulations = rest.iter().map(|term| {
            quote! {
                <() as ::oscen::graph::AccumulateEndpoints<_, _>>::accumulate(
                    &#term,
                    &mut #dst,
                );
            }
        });
        quote! {
            #lead
            #(#accumulations)*
        }
    }

    /// Bind a compound source to the destination endpoint's own projected
    /// type (`f32` for a mono input, `Frame<N>` for a frame input) so
    /// frame-returning calls and frame constructors route correctly while
    /// numeric literals still coerce. Falls back to an `f32` pin when the
    /// endpoint has no derive-emitted marker.
    fn compound_source_binding(&self, dest: &Address, src_tokens: &TokenStream) -> TokenStream {
        match self.endpoint_marker_tokens_for(dest.node, &dest.field) {
            Some((dst_path, dst_marker)) => quote! {
                let __src: <#dst_path as ::oscen::dispatch::EndpointAt<#dst_marker>>::Frame
                    = #src_tokens;
            },
            None => quote! { let __src: f32 = #src_tokens; },
        }
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
        let Some(node) = self.find_node_by_ident(node_name) else {
            return Vec::new();
        };
        let mut assignments = Vec::new();
        for group in self.ir.drivers.groups_for_node(node.id) {
            // Every driver of an accumulating group is same-rate (lowering
            // rejects anything else), so the first edge's kernel speaks for
            // the group.
            let lead_edge = &self.ir.edges[group.sources[0]];
            if !keep(&lead_edge.kernel) {
                continue;
            }
            if group.accumulates() {
                assignments.push(self.emit_accumulating_group(group));
            } else {
                assignments.push(self.emit_single_driver(lead_edge, &group.dest));
            }
        }
        assignments
    }

    /// Emit one edge driving one node destination. `dest` is the resolved
    /// address, so an indexed destination (`voices[2].input`) always
    /// addresses exactly that element, whatever the source's shape.
    fn emit_single_driver(&self, edge: &IrEdge, dest: &Address) -> TokenStream {
        let source = &edge.source;
        let dest_node = &self.ir.nodes[dest.node].name;
        let dest_field = &dest.field;

        // Compound sources (arithmetic, function/method calls) don't have a
        // single root endpoint. Evaluate them once and route via
        // ConnectEndpoints into the addressed slot(s).
        if !Self::is_simple_endpoint_source(source) {
            let src_tokens = self.emit_expr(source);
            return match dest.element {
                Element::All { n } => {
                    let src_binding = self.compound_source_binding(dest, &src_tokens);
                    quote! {
                        {
                            #src_binding
                            for i in 0..#n {
                                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                    &__src,
                                    &mut self.#dest_node[i].#dest_field,
                                );
                            }
                        }
                    }
                }
                Element::Index(_) => {
                    let src_binding = self.compound_source_binding(dest, &src_tokens);
                    let dst = self.emit_address(dest);
                    quote! {
                        {
                            #src_binding
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &__src,
                                &mut #dst,
                            );
                        }
                    }
                }
                Element::Scalar => {
                    let dst = self.emit_address(dest);
                    quote! {
                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                            &(#src_tokens),
                            &mut #dst,
                        );
                    }
                }
            };
        }

        let Some(source_ident) = self.extract_root_node(source) else {
            return quote! {};
        };
        let source_field = self.extract_endpoint_field(source);

        // Check if source is a graph input (not a node)
        let source_is_graph_input = self.is_input(source_ident);

        // Voice-allocator marker connections (`alloc.voices -> voices.x`):
        // element-wise routing of the allocator's per-voice event outputs.
        if let Some(field) = source_field {
            if *field == "voices" {
                return match dest.element {
                    Element::All { n } => quote! {
                        for i in 0..#n {
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &self.#source_ident.voices[i],
                                &mut self.#dest_node[i].#dest_field
                            );
                        }
                    },
                    _ => quote! {},
                };
            }
        }

        // An index on a node-array endpoint (`voices[0].output` /
        // `voices[2].frequency`) addresses a single element; such edges are
        // classified Scalar during lowering. Route them through the endpoint
        // emitters, which produce the `[k]` element access on the indexed
        // side.
        let src_elem_indexed = Self::ir_expr_as_endpoint(source)
            .and_then(|ep| ep.index)
            .filter(|_| self.get_node_array_size(source_ident).is_some())
            .is_some();
        if src_elem_indexed || matches!(dest.element, Element::Index(_)) {
            let src_toks = self.emit_expr(source);
            let dst_toks = self.emit_address(dest);
            return quote! {
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &#src_toks,
                    &mut #dst_toks
                );
            };
        }

        // A channel index on a scalar node's endpoint (`s.output[0]`)
        // extracts one channel of its `Frame<N>` value.
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

        match edge.fanout {
            FanoutShape::Scalar => {
                self.emit_scalar_connect(source_ident, &source_access, dest_node, dest_field)
            }
            FanoutShape::Parallel { n } => {
                self.emit_parallel_connect(source_ident, &source_access, dest_node, dest_field, n)
            }
            FanoutShape::Broadcast { n } => {
                self.emit_broadcast_connect(source_ident, &source_access, dest_node, dest_field, n)
            }
            FanoutShape::FanIn { n: _ } => {
                self.emit_fanin_connect(source_ident, source_field, dest_node, dest_field)
            }
        }
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

    /// Emit assignments for connections that target graph outputs, in
    /// canonical group order.
    pub(super) fn generate_graph_output_assignments_filtered<F>(&self, keep: F) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut out = Vec::new();
        for group in &self.ir.drivers.groups {
            let dest_ident = &self.ir.nodes[group.dest.node].name;
            let Some(output_kind) = self.output_kind(dest_ident) else {
                continue;
            };
            let lead_edge = &self.ir.edges[group.sources[0]];
            if !keep(&lead_edge.kernel) {
                continue;
            }
            if group.accumulates() {
                out.push(self.emit_accumulating_group(group));
                continue;
            }
            out.push(self.emit_single_output_driver(lead_edge, dest_ident, output_kind));
        }
        out
    }

    /// Emit one edge driving one graph output.
    fn emit_single_output_driver(
        &self,
        edge: &IrEdge,
        dest_ident: &syn::Ident,
        output_kind: EndpointKind,
    ) -> TokenStream {
        let source = &edge.source;
        let source_node = self.extract_root_node(source);
        let source_field = self.extract_endpoint_field(source);
        // Treat as "simple" only for a plain `node.field` endpoint
        // reference. Compound expressions (Binary, MethodCall) and bare
        // ident refs (graph inputs accessed without a dot-selector) are
        // not simple.
        let is_simple_source = Self::is_simple_endpoint_source(source) && source_field.is_some();

        // An index on the source endpoint selects one array element (or one
        // frame channel); it must never fan in over the whole array.
        let source_index = Self::ir_expr_as_endpoint(source).and_then(|ep| ep.index);

        match output_kind {
            EndpointKind::Stream | EndpointKind::Value => {
                if is_simple_source {
                    let source_node = source_node.unwrap();
                    let source_field = source_field.unwrap();
                    if self.get_node_array_size(source_node).is_some() && source_index.is_none() {
                        quote! {
                            self.#dest_ident = self.#source_node.iter().map(|n| n.#source_field).sum();
                        }
                    } else {
                        let source_tokens = self.emit_expr(source);
                        quote! {
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &#source_tokens,
                                &mut self.#dest_ident
                            );
                        }
                    }
                } else {
                    let source_tokens = self.emit_expr(source);
                    quote! {
                        self.#dest_ident = #source_tokens;
                    }
                }
            }
            EndpointKind::Event => {
                if is_simple_source {
                    let source_node = source_node.unwrap();
                    let source_field = source_field.unwrap();
                    let array_size = self
                        .get_node_array_size(source_node)
                        .filter(|_| source_index.is_none());
                    if let Some(array_size) = array_size {
                        quote! {
                            self.#dest_ident.clear();
                            for i in 0..#array_size {
                                for event in self.#source_node[i].#source_field.iter() {
                                    ::oscen::graph::debug_assert_event_pushed(
                                        self.#dest_ident.try_push(event.clone()),
                                    );
                                }
                            }
                        }
                    } else {
                        let source_tokens = self.emit_expr(source);
                        quote! {
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &#source_tokens,
                                &mut self.#dest_ident
                            );
                        }
                    }
                } else if Self::is_simple_endpoint_source(source) {
                    // Bare graph event input forwarded to a graph event
                    // output (`midi -> thru;`): copy the queue.
                    let source_tokens = self.emit_expr(source);
                    quote! {
                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                            &#source_tokens,
                            &mut self.#dest_ident
                        );
                    }
                } else {
                    quote! {}
                }
            }
            // Asset endpoints are bound from externals, never driven as a
            // graph output connection.
            EndpointKind::Asset => quote! {},
        }
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

        // Propagate through same-rate edges (including same-rate event edges)
        // until fixpoint. A compound source taints its destination if ANY
        // node it references is tainted, not just the leftmost — otherwise
        // `a.out + d.out -> mix.in` with a tainted `d` would leave `mix`
        // pre-inner, reading `d`'s previous-frame output.
        let mut changed = true;
        while changed {
            changed = false;
            for (_, edge) in self.edges() {
                if !is_same_rate_kernel(&edge.kernel) {
                    continue;
                }
                let dst = self.ir.nodes[edge.dest.node].name.to_string();
                if !same_rate(&dst) || tainted.contains(&dst) {
                    continue;
                }
                let src_tainted = crate::ir::lower::collect_referenced_node_ids(&edge.source)
                    .into_iter()
                    .any(|id| tainted.contains(&self.ir.nodes[id].name.to_string()));
                if src_tainted {
                    tainted.insert(dst);
                    changed = true;
                }
            }
        }

        // Diamond detection: any referenced node, not just the leftmost.
        for (_, edge) in self.edges() {
            if let EdgeKernel::Up { .. } = edge.kernel {
                let src_tainted = crate::ir::lower::collect_referenced_node_ids(&edge.source)
                    .into_iter()
                    .any(|id| tainted.contains(&self.ir.nodes[id].name.to_string()));
                {
                    if src_tainted {
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
