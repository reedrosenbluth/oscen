//! Frame orchestration. Emits the per-frame entry points
//! (`generate_advance_one_frame`) and decomposes the multirate inner-loop
//! body into per-bucket emitter methods (`emit_all_up_warmups`,
//! `emit_all_down_buffer_decls`, etc.).

use crate::ast::{EndpointKind, NodeRate};
use crate::ir::graph::{EdgeKernel, EventRescale, FanoutShape};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::Result;

use super::helpers::{down_buf_name, is_same_rate_kernel, resampler_field_name, up_buf_name};
use super::CodegenContext;

impl<'a> CodegenContext<'a> {
    // ========== Block Processing Methods ==========

    /// Generate the `__advance_one_frame()` private method.
    pub(super) fn generate_advance_one_frame(&self) -> Result<TokenStream> {
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

    // ========== Multirate Inner Body ==========

    /// Emit the multi-rate body.
    pub(super) fn generate_multirate_inner_body(&self) -> Result<TokenStream> {
        let max_factor = self.max_factor() as usize;

        let (pre_inner_outer_names, post_inner_outer_names, inner_node_names) =
            self.bucket_nodes_by_phase()?;

        let outer_process = self.emit_outer_processes(&pre_inner_outer_names);
        let post_inner_process = self.emit_post_inner_processes(&post_inner_outer_names);
        let up_decls = self.emit_all_up_warmups();
        let down_decls = self.emit_all_down_buffer_decls();
        let event_outer_drains = self.emit_all_event_outer_to_inner_drains();
        let event_inner_drains = self.emit_all_event_inner_to_outer_drains();
        let inner_event_calls = self.emit_all_inner_event_input_calls(&inner_node_names);
        let inner_writes = self.emit_all_inner_writes();
        let inner_processes = self.emit_all_inner_node_processes(&inner_node_names);
        let down_captures = self.emit_all_down_captures();
        let down_finalizes = self.emit_all_down_finalizes();
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
                #(#event_outer_drains)*

                // 6a. Run process_event_inputs() once per outer tick for inner nodes.
                #(#inner_event_calls)*

                // 6. Inner loop: ×N nodes run N times per outer tick.
                for __inner in 0..#max_factor {
                    #(#inner_writes)*
                    #(#inner_processes)*
                    #(#down_captures)*
                }

                // 7. Downsample once per outer tick into dest fields.
                #(#down_finalizes)*

                // 7a. Cross-rate event drains: inner -> outer.
                #(#event_inner_drains)*

                // 7.5. Post-inner outer-rate nodes.
                #(#post_inner_process)*

                // 8. Same-rate trailer assignments (e.g., to graph outputs).
                #(#same_rate_output_trailer)*
            }
        })
    }

    // ========== Node bucketing ==========

    /// Partition nodes into (pre_inner_outer, post_inner_outer, inner) buckets.
    ///
    /// Returns `(pre_inner_outer_names, post_inner_outer_names, inner_node_names)`.
    fn bucket_nodes_by_phase(&self) -> Result<(Vec<syn::Ident>, Vec<syn::Ident>, Vec<syn::Ident>)> {
        let sorted_nodes: Vec<syn::Ident> = self.nodes().map(|n| n.name.clone()).collect();

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

        let tainted: HashSet<String> = self.compute_post_inner_same_nodes()?;

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

        Ok((
            pre_inner_outer_names,
            post_inner_outer_names,
            inner_node_names,
        ))
    }

    // ========== Per-bucket emitters ==========

    /// Step 3: Outer-rate (pre-inner) node process calls.
    fn emit_outer_processes(&self, node_names: &[syn::Ident]) -> Vec<TokenStream> {
        let mut out = Vec::new();
        for node_name in node_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            out.extend(assignments);
            out.push(self.emit_node_process_call(node_name));
        }
        out
    }

    /// Step 7.5: Post-inner outer-rate node process calls.
    fn emit_post_inner_processes(&self, node_names: &[syn::Ident]) -> Vec<TokenStream> {
        let mut out = Vec::new();
        for node_name in node_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            out.extend(assignments);
            out.push(self.emit_node_process_call(node_name));
        }
        out
    }

    /// Step 4: Per-edge upsample warmup declarations for `EdgeKernel::Up` edges.
    fn emit_all_up_warmups(&self) -> Vec<TokenStream> {
        let mut decls = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Up { factor, .. } = edge.kernel {
                decls.push(self.emit_up_warmup_for_edge(idx, edge, factor));
            }
        }
        decls
    }

    fn emit_up_warmup_for_edge(
        &self,
        idx: usize,
        edge: &crate::ir::graph::IrEdge,
        factor: u32,
    ) -> TokenStream {
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
            let source_ident = self
                .extract_root_node(&edge.source)
                .expect("Parallel edge has array root");
            let source_field = self
                .extract_endpoint_field(&edge.source)
                .expect("Parallel edge has field access");
            quote! {
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
            }
        } else {
            let src_value = self.connection_source_value_expr(&edge.source);
            quote! {
                let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
                {
                    let __src_val: f32 = #src_value;
                    ::oscen::resample::StreamUpsampler::upsample(
                        &mut self.#field #access,
                        __src_val,
                        &mut #buf,
                    );
                }
            }
        }
    }

    /// Step 5: Per-edge accumulator buffer declarations for `EdgeKernel::Down` edges.
    fn emit_all_down_buffer_decls(&self) -> Vec<TokenStream> {
        let mut decls = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { factor, .. } = edge.kernel {
                decls.push(self.emit_down_buffer_decl_for_edge(idx, edge, factor));
            }
        }
        decls
    }

    fn emit_down_buffer_decl_for_edge(
        &self,
        idx: usize,
        edge: &crate::ir::graph::IrEdge,
        factor: u32,
    ) -> TokenStream {
        let factor_us = factor as usize;
        let buf = down_buf_name(idx);
        if let FanoutShape::Parallel { n } = edge.fanout {
            quote! {
                let mut #buf: [[f32; #factor_us]; #n] = [[0.0; #factor_us]; #n];
            }
        } else {
            quote! {
                let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
            }
        }
    }

    /// Step 5.5: Cross-rate event drains: outer → inner (`Multiply` rescale).
    fn emit_all_event_outer_to_inner_drains(&self) -> Vec<TokenStream> {
        let mut drains = Vec::new();
        for (_, edge) in self.edges() {
            if let EdgeKernel::Event {
                rescale: EventRescale::Multiply(n),
            } = edge.kernel
            {
                drains.push(self.generate_event_drain(
                    &edge.source,
                    &edge.dest,
                    EventRescale::Multiply(n),
                ));
            }
        }
        drains
    }

    /// Step 7a: Cross-rate event drains: inner → outer (`Divide` rescale).
    fn emit_all_event_inner_to_outer_drains(&self) -> Vec<TokenStream> {
        let mut drains = Vec::new();
        for (_, edge) in self.edges() {
            if let EdgeKernel::Event {
                rescale: EventRescale::Divide(n),
            } = edge.kernel
            {
                drains.push(self.generate_event_drain(
                    &edge.source,
                    &edge.dest,
                    EventRescale::Divide(n),
                ));
            }
        }
        drains
    }

    /// Step 6a: `process_event_inputs()` calls for inner-rate nodes (once per outer tick).
    fn emit_all_inner_event_input_calls(
        &self,
        inner_node_names: &[syn::Ident],
    ) -> Vec<TokenStream> {
        inner_node_names
            .iter()
            .map(|n| self.emit_node_process_event_inputs(n))
            .collect()
    }

    /// Step 6 (inner loop part A): Write upsampled data into inner-rate node inputs.
    fn emit_all_inner_writes(&self) -> Vec<TokenStream> {
        let mut writes = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Up { .. } = edge.kernel {
                let buf = up_buf_name(idx);

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let dest_node = &self.ir.nodes[edge.dest.node].name;
                    let dest_field = &edge.dest.endpoint;
                    writes.push(quote! {
                        for __k in 0..#n {
                            let __dst_val: f32 = #buf[__k][__inner];
                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                &__dst_val,
                                &mut self.#dest_node[__k].#dest_field,
                            );
                        }
                    });
                } else {
                    let dest_assign =
                        self.connection_dest_field_assign(&edge.dest, &quote! { #buf[__inner] });
                    writes.push(dest_assign);
                }
            }
        }
        writes
    }

    /// Step 6 (inner loop part B): Inner-rate node connection assignments and process calls.
    fn emit_all_inner_node_processes(&self, inner_node_names: &[syn::Ident]) -> Vec<TokenStream> {
        let mut runs = Vec::new();
        for node_name in inner_node_names {
            let assignments = self
                .generate_connection_assignments_for_node_filtered(node_name, is_same_rate_kernel);
            runs.extend(assignments);
            runs.push(self.emit_node_process_only(node_name));
        }
        runs
    }

    /// Step 6 (inner loop part C): Capture inner-rate outputs into down-accumulator buffers.
    fn emit_all_down_captures(&self) -> Vec<TokenStream> {
        let mut captures = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { .. } = edge.kernel {
                let buf = down_buf_name(idx);

                if let FanoutShape::Parallel { n } = edge.fanout {
                    let source_ident = self
                        .extract_root_node(&edge.source)
                        .expect("Parallel edge has array root");
                    let source_field = self
                        .extract_endpoint_field(&edge.source)
                        .expect("Parallel edge has field access");
                    captures.push(quote! {
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
                    let src_value = self.connection_source_value_expr(&edge.source);
                    captures.push(quote! {
                        #buf[__inner] = #src_value;
                    });
                }
            }
        }
        captures
    }

    /// Step 7: Finalize `Down` edges: run downsampler and write to dest fields.
    fn emit_all_down_finalizes(&self) -> Vec<TokenStream> {
        let mut finalizes = Vec::new();
        for (idx, edge) in self.edges() {
            if let EdgeKernel::Down { .. } = edge.kernel {
                finalizes.push(self.emit_down_finalize_for_edge(idx, edge));
            }
        }
        finalizes
    }

    fn emit_down_finalize_for_edge(
        &self,
        idx: usize,
        edge: &crate::ir::graph::IrEdge,
    ) -> TokenStream {
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
            quote! {
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
            }
        } else {
            self.connection_dest_field_assign(
                &edge.dest,
                &quote! {
                    ::oscen::resample::StreamDownsampler::downsample(
                        &mut self.#field #access,
                        &#buf,
                    )
                },
            )
        }
    }
}
