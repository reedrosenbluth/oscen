//! Struct generation: type definition, field declarations, new() constructor.
//!
//! Methods here emit the `pub struct GraphName { ... }` declaration, the
//! `pub fn new()` constructor body (input/output/node var init and `Self {
//! ... }`), cross-rate resampler fields and their initializers, and the
//! per-node `set_sample_rate()`/`prepare()` calls emitted inside the graph's
//! `set_sample_rate` and `SignalProcessor::prepare`.

use crate::ast::{EndpointKind, NodeRate};
use crate::ir::graph::{EdgeKernel, FanoutShape, IrNodeKind, NodeId};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::Expr;

use super::helpers::{kernel_down_type, kernel_up_type, policy_marker_path, resampler_field_name};
use super::CodegenContext;

impl<'a> CodegenContext<'a> {
    /// Generate static initialization for input parameters.
    pub(super) fn generate_static_input_params(&self) -> Vec<TokenStream> {
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
                    // Assets are externals, not graph inputs — no init here.
                    EndpointKind::Asset => {}
                }
                stmts
            })
            .collect()
    }

    /// Generate static initialization for output parameters.
    /// For static graphs, outputs store actual values (f32) not endpoint wrappers.
    pub(super) fn generate_static_output_params(&self) -> Vec<TokenStream> {
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
                    // Assets are externals, not graph outputs — no init here.
                    EndpointKind::Asset => {}
                }
                stmts
            })
            .collect()
    }

    /// Generate static initialization for nodes (direct constructor calls).
    pub(super) fn generate_static_node_init(&self) -> Vec<TokenStream> {
        // Asset-bound nodes need a `mut` binding so the generated wiring can
        // call `install_asset(&mut node, ...)` before the node is moved into
        // `Self`.
        let asset_bound: HashSet<NodeId> = self.ir.asset_bindings.iter().map(|b| b.node).collect();
        self.nodes()
            .map(|node| {
                let name = &node.name;
                let binding = if asset_bound.contains(&node.id) {
                    quote! { let mut #name }
                } else {
                    quote! { let #name }
                };
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
                        #binding = [#(#constructors),*];
                    }
                } else {
                    // Single node initialization
                    quote! {
                        #binding = #constructor;
                    }
                }
            })
            .collect()
    }

    /// Generate static struct initialization (includes sample_rate, nodes - no IO fields).
    pub(super) fn generate_static_struct_init(&self) -> TokenStream {
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

        // Add node fields (no IO fields), then asset load-handle fields. Both
        // are plain locals bound in `new()` before the `Self { .. }` literal.
        let mut node_fields: Vec<_> = self
            .nodes()
            .map(|node| {
                let name = &node.name;
                quote! { #name }
            })
            .collect();
        for binding in &self.ir.asset_bindings {
            let name = &binding.external_name;
            node_fields.push(quote! { #name });
        }

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

    /// Emit a const-time `T: ::oscen::AllowsFeedback` bound assertion for the
    /// source's primary node on every feedback edge in the IR (the outgoing
    /// leg of an inline-delay `-> [N] ->` / `-> [name] ->` expansion).
    ///
    /// A user-defined source type that doesn't impl `AllowsFeedback` fails
    /// to compile, with the error span pointing at the connection
    /// statement.
    pub(super) fn generate_feedback_assertions(&self) -> Vec<TokenStream> {
        let mut out = Vec::new();
        for edge in self.ir.edges.values() {
            if !edge.is_feedback {
                continue;
            }
            let Some(primary) = crate::ir::expr::primary_node(&edge.source) else {
                continue;
            };
            let node = &self.ir.nodes[primary];
            let path = match &node.kind {
                IrNodeKind::Processor { ty: Some(p), .. }
                | IrNodeKind::NodeArray { ty: Some(p), .. } => p,
                _ => continue,
            };
            let span = edge.span;
            let assertion = quote::quote_spanned! { span =>
                #[allow(non_snake_case)]
                const _: fn() = || {
                    fn _assert_allows_feedback<T: ::oscen::graph::AllowsFeedback>() {}
                    _assert_allows_feedback::<#path>();
                };
            };
            out.push(assertion);
        }
        out
    }

    /// Emit a `quote_spanned!`-spanned const-time trait-bound assertion per
    /// cross-rate edge whose source and destination are both projectable.
    pub(super) fn generate_kind_assertions(&self) -> Vec<TokenStream> {
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
    pub(super) fn generate_resampler_fields(&self) -> Vec<TokenStream> {
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
    pub(super) fn generate_resampler_inits(&self) -> Vec<TokenStream> {
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

    /// Generate per-node `prepare()` calls. Rate distribution happens
    /// separately (and first) via the calls from
    /// [`generate_node_set_sample_rate_calls`], so `prepare` takes no rate —
    /// each node reads its own `SampleRate` field.
    pub(super) fn generate_node_prepare_calls(&self) -> Vec<TokenStream> {
        let mut calls = Vec::new();
        for node in self.nodes() {
            let name = &node.name;
            let is_array = matches!(node.kind, IrNodeKind::NodeArray { .. });
            if is_array {
                calls.push(quote! {
                    for __child in self.#name.iter_mut() {
                        ::oscen::SignalProcessor::prepare(__child);
                    }
                });
            } else {
                calls.push(quote! {
                    ::oscen::SignalProcessor::prepare(&mut self.#name);
                });
            }
        }
        calls
    }

    /// Generate per-node `set_sample_rate()` calls that scale `sample_rate` by
    /// the node's rate annotation. Emitted into the graph's `set_sample_rate`
    /// so a rate change reaches every child (and, recursively, nested graphs)
    /// without touching any other state — `init` is what resets resamplers and
    /// recomputes derived state.
    pub(super) fn generate_node_set_sample_rate_calls(&self) -> Vec<TokenStream> {
        let mut calls = Vec::new();
        for node in self.nodes() {
            let name = &node.name;
            let scaled = scaled_rate_expr(node.rate);
            let is_array = matches!(node.kind, IrNodeKind::NodeArray { .. });
            if is_array {
                calls.push(quote! {
                    for __child in self.#name.iter_mut() {
                        __child.set_sample_rate(#scaled);
                    }
                });
            } else {
                calls.push(quote! {
                    self.#name.set_sample_rate(#scaled);
                });
            }
        }
        calls
    }

    /// Generate the `new()`-body wiring for each `external -> node.asset`
    /// binding: a handoff `pair`, `install_asset` into the (mut) node, and the
    /// `let <name> = AssetLoadHandle::new(..)` local consumed by `Self { .. }`.
    pub(super) fn generate_asset_wiring(&self) -> Vec<TokenStream> {
        self.ir
            .asset_bindings
            .iter()
            .map(|binding| {
                let field = &binding.external_name;
                let node = &self.ir.nodes[binding.node];
                let node_name = &node.name;
                // Guaranteed by lower: asset targets are typed processor nodes.
                let node_ty = self
                    .node_type_path(node)
                    .expect("asset-bound node must have a known type path");
                let pub_ident = syn::Ident::new(&format!("__{}_pub", field), field.span());
                let con_ident = syn::Ident::new(&format!("__{}_con", field), field.span());
                quote! {
                    let (#pub_ident, #con_ident) = ::oscen::handoff::pair::<
                        <<#node_ty as ::oscen::asset::AssetEndpoint>::Consumer
                            as ::oscen::asset::AssetConsumer>::Playable,
                    >();
                    <#node_ty as ::oscen::asset::AssetEndpoint>::install_asset(
                        &mut #node_name,
                        #con_ident,
                    );
                    let #field = ::oscen::asset::AssetLoadHandle::new(
                        #pub_ident,
                        <#node_ty as ::oscen::asset::AssetEndpoint>::asset_builder(),
                    );
                }
            })
            .collect()
    }

    /// Generate the struct field for each asset load handle.
    pub(super) fn generate_asset_handle_fields(&self) -> Vec<TokenStream> {
        self.ir
            .asset_bindings
            .iter()
            .map(|binding| {
                let field = &binding.external_name;
                let node = &self.ir.nodes[binding.node];
                let node_ty = self
                    .node_type_path(node)
                    .expect("asset-bound node must have a known type path");
                quote! {
                    pub #field: ::oscen::asset::AssetLoadHandle<
                        <#node_ty as ::oscen::asset::AssetEndpoint>::Consumer,
                    >
                }
            })
            .collect()
    }

    /// Generate `self.<name>.set_graph_rate(sample_rate as u32);` for each
    /// asset handle, emitted into the graph's `set_sample_rate`.
    pub(super) fn generate_asset_set_graph_rate_calls(&self) -> Vec<TokenStream> {
        self.ir
            .asset_bindings
            .iter()
            .map(|binding| {
                let field = &binding.external_name;
                quote! { self.#field.set_graph_rate(sample_rate as u32); }
            })
            .collect()
    }

    /// Generate `reset()` calls for every cross-rate resampler kernel.
    pub(super) fn generate_resampler_resets(&self) -> Vec<TokenStream> {
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
    pub(super) fn generate_latency_method(&self) -> TokenStream {
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
}

/// Expression for a node's effective rate given the graph-level `sample_rate`
/// binding in scope, scaled by the node's rate annotation.
fn scaled_rate_expr(rate: NodeRate) -> TokenStream {
    match rate {
        NodeRate::Same => quote! { sample_rate },
        NodeRate::Up(f) => {
            let f = f as f32;
            quote! { sample_rate * #f }
        }
        NodeRate::Down(d) => {
            let d = d as f32;
            quote! { sample_rate / #d }
        }
    }
}
