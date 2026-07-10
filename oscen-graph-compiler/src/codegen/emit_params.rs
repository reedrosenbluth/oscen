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
//! allocation-free). Hoisted value inputs without an explicit `= default`
//! inherit their initial value from the child constructor in `new()`
//! (`generate_hoist_default_inherits`); the descriptor table reports the same
//! value by constructing a probe graph inside the lazy init and reading it
//! back via `get_param`.

use crate::ast::Curve;
use crate::ir::graph::IrNode;
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use super::helpers::{camel_case, ident_base, title_case};
use super::CodegenContext;

/// The param-enum variant for a value input: its UpperCamelCase form,
/// validated. `camel_case` can produce non-identifiers — `""` from `__`,
/// the keyword `Self` from `self_`/`_self` — which used to panic
/// `Ident::new` or emit invalid Rust; both are spanned errors instead.
/// A raw-ident input (`r#loop`) camel-cases its bare name (`Loop`).
fn variant_ident(name: &syn::Ident) -> syn::Result<syn::Ident> {
    let base = name.to_string();
    let base = base.strip_prefix("r#").unwrap_or(&base);
    let camel = camel_case(base);
    syn::parse_str::<syn::Ident>(&camel)
        .map(|mut id| {
            id.set_span(name.span());
            id
        })
        .map_err(|_| {
            syn::Error::new(
                name.span(),
                format!(
                    "value input `{name}` maps to parameter enum variant `{camel}`, \
                     which is not a valid identifier; rename the input so its \
                     UpperCamelCase form is a plain Rust identifier"
                ),
            )
        })
}

impl<'a> CodegenContext<'a> {
    /// Emit the parameter registry items (enum + descriptor table +
    /// dispatchers). Returns an empty stream when the graph has no f32
    /// value inputs, and an error when two input names collapse to the same
    /// UpperCamelCase enum variant.
    ///
    /// The registry is built from the *params* partition of the value
    /// inputs only (`param_value_inputs`): TYPED value inputs (declared
    /// with a non-f32 type) have no enum variant, descriptor, or
    /// `set_param`/`get_param` arm — their only surface is the typed
    /// setter. The nih-plug wrapper indexes `param_descriptors()`
    /// positionally, so it must (and does) use the same filtered list.
    pub(super) fn generate_param_registry(&self) -> syn::Result<TokenStream> {
        let graph_name = self.name();
        let value_inputs: Vec<&IrNode> = self.param_value_inputs();
        if value_inputs.is_empty() {
            return Ok(quote! {});
        }

        let enum_name = syn::Ident::new(&format!("{}Param", graph_name), graph_name.span());
        let count = value_inputs.len();

        let mut variants: Vec<syn::Ident> = Vec::with_capacity(value_inputs.len());
        let mut variant_err: Option<syn::Error> = None;
        for n in &value_inputs {
            match variant_ident(&n.name) {
                Ok(v) => variants.push(v),
                Err(e) => match variant_err.as_mut() {
                    Some(acc) => acc.combine(e),
                    None => variant_err = Some(e),
                },
            }
        }
        if let Some(err) = variant_err {
            return Err(err);
        }
        // Bare names (no `r#`): the registry's string surface (`name()`,
        // `from_name`) matches the setter/variant naming.
        let name_strs: Vec<String> = value_inputs.iter().map(|n| ident_base(&n.name)).collect();

        // `camel_case` collapses underscore placement, so distinct input
        // names (e.g. `oscA_pitch` and `osc_a_pitch`) can map to the same
        // enum variant. Catch that here with a spanned error instead of
        // letting rustc report a duplicate variant the user never wrote.
        let mut seen_variants: HashMap<String, &syn::Ident> = HashMap::new();
        let mut collision_err: Option<syn::Error> = None;
        for (node, variant) in value_inputs.iter().zip(&variants) {
            match seen_variants.entry(variant.to_string()) {
                Entry::Occupied(first) => {
                    let msg = format!(
                        "value inputs `{}` and `{}` both map to parameter enum variant \
                         `{}::{}`; rename one so their UpperCamelCase forms differ",
                        first.get(),
                        node.name,
                        enum_name,
                        variant,
                    );
                    let mut err = syn::Error::new(node.name.span(), &msg);
                    err.combine(syn::Error::new(first.get().span(), &msg));
                    match collision_err.as_mut() {
                        Some(acc) => acc.combine(err),
                        None => collision_err = Some(err),
                    }
                }
                Entry::Vacant(slot) => {
                    slot.insert(&node.name);
                }
            }
        }
        if let Some(err) = collision_err {
            return Err(err);
        }

        // Hoisted inputs without an explicit `= default` inherit their
        // initial value from the child constructor in `new()`. The
        // descriptor table must report that same value, so when any such
        // input exists the lazy init constructs a probe graph and reads
        // the inherited defaults back through `get_param`.
        let needs_probe = value_inputs
            .iter()
            .any(|n| self.input_default(n).is_none() && self.input_hoist(n).is_some());
        let probe_init = if needs_probe {
            quote! { let __probe = #graph_name::new(); }
        } else {
            quote! {}
        };

        // ---- descriptor table entries -----------------------------------
        let descriptors: Vec<TokenStream> = value_inputs
            .iter()
            .enumerate()
            .map(|(idx, node)| {
                let name_str = ident_base(&node.name);
                let spec = self.input_spec(node);
                let display_name = spec
                    .and_then(|s| s.display_name.clone())
                    .unwrap_or_else(|| title_case(&name_str));
                let default = match self.input_default(node) {
                    Some(e) => quote! { (#e) as f32 },
                    None if self.input_hoist(node).is_some() => {
                        // Inherited from the child constructor: read it off
                        // the probe instance so the descriptor matches what
                        // `get_param` returns right after `new()`.
                        let variant = &variants[idx];
                        quote! { __probe.get_param(#enum_name::#variant) }
                    }
                    None => quote! { 0.0f32 },
                };
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
                let logarithmic = matches!(spec.and_then(|s| s.curve), Some(Curve::Logarithmic));

                quote! {
                    ::oscen::graph::ParamDescriptor {
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
        // The probe graph can be multi-megabyte (voice arrays, block
        // buffers) and `param_descriptors()` is reached implicitly from
        // arbitrary host threads (nih-plug's `Params::default()`), whose
        // stacks are often ~1 MB. Build the table on a dedicated big-stack
        // thread; fall back inline where spawning isn't available (wasm).
        // `Box::new(new())` would not help: the argument is still
        // constructed on the caller's stack first.
        let descriptor_init = if needs_probe {
            quote! {
                fn __build() -> [::oscen::graph::ParamDescriptor; #count] {
                    #probe_init
                    [
                        #(#descriptors,)*
                    ]
                }
                ::std::thread::Builder::new()
                    .name("oscen-param-descriptors".into())
                    .stack_size(16 * 1024 * 1024)
                    .spawn(__build)
                    .ok()
                    .and_then(|h| h.join().ok())
                    .unwrap_or_else(__build)
            }
        } else {
            quote! {
                #probe_init
                [
                    #(#descriptors,)*
                ]
            }
        };

        // ---- dispatch arms -----------------------------------------------
        let set_arms: Vec<TokenStream> = value_inputs
            .iter()
            .zip(&variants)
            .map(|(node, variant)| {
                let name = &node.name;
                let set_name = syn::Ident::new(&format!("set_{}", ident_base(name)), name.span());
                quote! { #enum_name::#variant => self.#set_name(value), }
            })
            .collect();

        let set_immediate_arms: Vec<TokenStream> = value_inputs
            .iter()
            .zip(&variants)
            .map(|(node, variant)| {
                let name = &node.name;
                let setter = if self.is_ramped_input(name).is_some() {
                    syn::Ident::new(&format!("set_{}_immediate", ident_base(name)), name.span())
                } else {
                    syn::Ident::new(&format!("set_{}", ident_base(name)), name.span())
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

        Ok(quote! {
            /// Parameter identifiers for the value inputs of the generated
            /// graph, in declaration order. Generated by `graph!`.
            /// `#[repr(usize)]` assigns discriminants 0..N in declaration
            /// order, which is also each parameter's descriptor-table index.
            #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
            #[repr(usize)]
            #[allow(dead_code)]
            pub enum #enum_name {
                #(#variants,)*
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
                /// Graphs with hoist-inherited defaults build the table on an
                /// internal big-stack thread, so small-stack host threads are
                /// safe.
                pub fn param_descriptors() -> &'static [::oscen::graph::ParamDescriptor] {
                    static DESCRIPTORS: ::std::sync::OnceLock<
                        [::oscen::graph::ParamDescriptor; #count],
                    > = ::std::sync::OnceLock::new();
                    DESCRIPTORS.get_or_init(|| {
                        #descriptor_init
                    })
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
        })
    }
}
