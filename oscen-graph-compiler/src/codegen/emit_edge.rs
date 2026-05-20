//! Per-edge emitters. Each method emits the TokenStream for one edge's
//! contribution to either the same-rate path or one stage of the
//! multi-rate inner loop.

use crate::ir::graph::EventRescale;
use proc_macro2::TokenStream;
use quote::quote;

use super::CodegenContext;

impl<'a> CodegenContext<'a> {
    /// Same-rate Scalar → Scalar connection via `ConnectEndpoints`.
    pub(super) fn emit_scalar_connect(
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
    pub(super) fn emit_parallel_connect(
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
    pub(super) fn emit_broadcast_connect(
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
    pub(super) fn emit_fanin_connect(
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

    /// Emit the cross-rate event drain for one edge.
    pub(super) fn generate_event_drain(
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

    /// Build an `f32`-valued expression for a connection's source.
    pub(super) fn connection_source_value_expr(
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
    pub(super) fn connection_dest_field_assign(
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
}
