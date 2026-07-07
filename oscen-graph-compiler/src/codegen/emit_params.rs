//! Parameter registry generation.
//!
//! For every graph with at least one value input, emit:
//! - a `{Graph}Param` C-like enum (one variant per value input, declaration
//!   order),
//! - `{Graph}::param_descriptors()` returning a `&'static [ParamDescriptor]`
//!   table with name/default/range/unit/ramp metadata,
//! - RT-safe `set_param` / `set_param_immediate` / `get_param` dispatchers.
//!
//! The descriptor table is the target-independent single source of truth for
//! parameter metadata: preset systems, standalone UIs, and plugin wrappers
//! consume it instead of re-declaring ranges by hand. It is built lazily in a
//! `OnceLock` so default/range expressions may be arbitrary runtime `Expr`s
//! (call `param_descriptors()` off the audio thread; the enum dispatchers are
//! allocation-free).

use crate::ast::{Curve, EndpointKind};
use crate::ir::graph::IrNode;
use proc_macro2::TokenStream;
use quote::quote;

use super::helpers::title_case;
use super::CodegenContext;

/// snake_case -> UpperCamelCase for enum variant names.
fn camel_case(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

impl<'a> CodegenContext<'a> {
    /// Emit the parameter registry items (enum + descriptor table +
    /// dispatchers). Returns an empty stream when the graph has no value
    /// inputs.
    pub(super) fn generate_param_registry(&self) -> TokenStream {
        let graph_name = self.name();
        let value_inputs: Vec<&IrNode> = self
            .inputs()
            .filter(|n| matches!(self.input_kind(&n.name), Some(EndpointKind::Value)))
            .collect();
        if value_inputs.is_empty() {
            return quote! {};
        }

        let enum_name = syn::Ident::new(&format!("{}Param", graph_name), graph_name.span());
        let count = value_inputs.len();

        let variants: Vec<syn::Ident> = value_inputs
            .iter()
            .map(|n| syn::Ident::new(&camel_case(&n.name.to_string()), n.name.span()))
            .collect();
        let name_strs: Vec<String> = value_inputs.iter().map(|n| n.name.to_string()).collect();

        // ---- descriptor table entries -----------------------------------
        let descriptors: Vec<TokenStream> = value_inputs
            .iter()
            .enumerate()
            .map(|(idx, node)| {
                let name_str = node.name.to_string();
                let spec = self.input_spec(node);
                let display_name = spec
                    .and_then(|s| s.display_name.clone())
                    .unwrap_or_else(|| title_case(&name_str));
                let default = self
                    .input_default(node)
                    .map(|e| quote! { (#e) as f32 })
                    .unwrap_or_else(|| quote! { 0.0f32 });
                let range = spec
                    .and_then(|s| s.range.as_ref())
                    .map(|r| {
                        let min = &r.min;
                        let max = &r.max;
                        quote! { Some(((#min) as f32, (#max) as f32)) }
                    })
                    .unwrap_or_else(|| quote! { None });
                let center = spec
                    .and_then(|s| s.center.as_ref())
                    .map(|c| quote! { Some((#c) as f32) })
                    .unwrap_or_else(|| quote! { None });
                let unit = spec
                    .and_then(|s| s.unit.as_ref())
                    .map(|u| quote! { Some(#u) })
                    .unwrap_or_else(|| quote! { None });
                let ramp = spec
                    .and_then(|s| s.ramp)
                    .map(|r| quote! { Some(#r as u32) })
                    .unwrap_or_else(|| quote! { None });
                let step = spec
                    .and_then(|s| s.step.as_ref())
                    .map(|s| quote! { Some((#s) as f32) })
                    .unwrap_or_else(|| quote! { None });
                let group = spec
                    .and_then(|s| s.group.as_ref())
                    .map(|g| quote! { Some(#g) })
                    .unwrap_or_else(|| quote! { None });
                let logarithmic = matches!(
                    spec.and_then(|s| s.curve),
                    Some(Curve::Logarithmic)
                );

                quote! {
                    ::oscen::graph::ParamDescriptor {
                        index: #idx,
                        name: #name_str,
                        display_name: #display_name,
                        default: #default,
                        range: #range,
                        center: #center,
                        unit: #unit,
                        ramp_frames: #ramp,
                        step: #step,
                        group: #group,
                        logarithmic: #logarithmic,
                    }
                }
            })
            .collect();

        // ---- dispatch arms -----------------------------------------------
        let set_arms: Vec<TokenStream> = value_inputs
            .iter()
            .zip(&variants)
            .map(|(node, variant)| {
                let name = &node.name;
                let set_name = syn::Ident::new(&format!("set_{}", name), name.span());
                quote! { #enum_name::#variant => self.#set_name(value), }
            })
            .collect();

        let set_immediate_arms: Vec<TokenStream> = value_inputs
            .iter()
            .zip(&variants)
            .map(|(node, variant)| {
                let name = &node.name;
                let setter = if self.is_ramped_input(name).is_some() {
                    syn::Ident::new(&format!("set_{}_immediate", name), name.span())
                } else {
                    syn::Ident::new(&format!("set_{}", name), name.span())
                };
                quote! { #enum_name::#variant => self.#setter(value), }
            })
            .collect();

        let get_arms: Vec<TokenStream> = value_inputs
            .iter()
            .zip(&variants)
            .map(|(node, variant)| {
                let name = &node.name;
                if self.is_ramped_input(name).is_some() {
                    quote! { #enum_name::#variant => self.#name.target, }
                } else {
                    quote! { #enum_name::#variant => self.#name, }
                }
            })
            .collect();

        let from_name_arms: Vec<TokenStream> = name_strs
            .iter()
            .zip(&variants)
            .map(|(s, v)| quote! { #s => Some(#enum_name::#v), })
            .collect();

        let indices: Vec<usize> = (0..count).collect();

        quote! {
            /// Parameter identifiers for the value inputs of the generated
            /// graph, in declaration order. Generated by `graph!`.
            #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
            #[repr(usize)]
            #[allow(dead_code)]
            pub enum #enum_name {
                #(#variants = #indices,)*
            }

            #[allow(dead_code)]
            impl #enum_name {
                /// Every parameter, in declaration order.
                pub const ALL: [#enum_name; #count] = [
                    #(#enum_name::#variants,)*
                ];

                /// Number of parameters.
                pub const COUNT: usize = #count;

                /// Declaration-order index (also the descriptor-table index).
                #[inline]
                pub fn index(self) -> usize {
                    self as usize
                }

                /// The input's field name, e.g. `"osc_a_pitch"`.
                pub fn name(self) -> &'static str {
                    Self::NAMES[self as usize]
                }

                const NAMES: [&'static str; #count] = [
                    #(#name_strs,)*
                ];

                /// Look up a parameter by its field name. Not RT-safe-critical
                /// but allocation-free.
                pub fn from_name(name: &str) -> Option<Self> {
                    match name {
                        #(#from_name_arms)*
                        _ => None,
                    }
                }

                /// Static metadata for this parameter.
                pub fn descriptor(self) -> &'static ::oscen::graph::ParamDescriptor {
                    &#graph_name::param_descriptors()[self as usize]
                }
            }

            #[allow(dead_code)]
            impl #graph_name {
                /// Metadata for every value input ("parameter") of this graph,
                /// in declaration order. Built lazily on first call — call it
                /// off the audio thread (e.g. during editor/preset setup).
                pub fn param_descriptors() -> &'static [::oscen::graph::ParamDescriptor] {
                    static DESCRIPTORS: ::std::sync::OnceLock<
                        [::oscen::graph::ParamDescriptor; #count],
                    > = ::std::sync::OnceLock::new();
                    DESCRIPTORS.get_or_init(|| [
                        #(#descriptors,)*
                    ])
                }

                /// Set a parameter by id (uses the input's default ramp when
                /// one is declared). RT-safe: no allocation or locking.
                #[inline]
                pub fn set_param(&mut self, param: #enum_name, value: f32) {
                    match param {
                        #(#set_arms)*
                    }
                }

                /// Set a parameter by id, bypassing any declared ramp.
                /// RT-safe: no allocation or locking.
                #[inline]
                pub fn set_param_immediate(&mut self, param: #enum_name, value: f32) {
                    match param {
                        #(#set_immediate_arms)*
                    }
                }

                /// Read a parameter's current target value by id.
                #[inline]
                pub fn get_param(&self, param: #enum_name) -> f32 {
                    match param {
                        #(#get_arms)*
                    }
                }
            }
        }
    }
}
