//! Per-node emitters: outer/inner process calls, event input dispatch,
//! taint analysis, and per-node incoming-edge assignment.

use crate::ast::{EndpointKind, NodeRate};
use crate::ir::graph::{EdgeKernel, EventRescale, FanoutShape};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::Result;

use super::helpers::root_node_name;
use super::CodegenContext;

impl<'a> CodegenContext<'a> {
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
