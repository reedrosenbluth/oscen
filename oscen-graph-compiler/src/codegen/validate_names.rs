//! Generated-API name-collision validation.
//!
//! Every graph declaration becomes a struct field, and most also derive
//! inherent methods (`set_<name>`, `push_<name>`, `<name>_block`, ...) on
//! the graph type — alongside a fixed built-in surface (`process`,
//! `set_sample_rate`, `set_param`, ...). Without this pass, an input named
//! `param` or `sample_rate` compiles into duplicate inherent methods and
//! the user gets rustc's E0592 pointing at code they never wrote. Fields
//! and methods live in separate namespaces, so each is tracked separately.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::ast::EndpointKind;

use super::CodegenContext;

/// Inherent methods (and associated fns — same namespace) that codegen
/// always or conditionally emits on the graph struct with fixed names.
const FIXED_METHODS: &[&str] = &[
    "new",
    "init",
    "set_sample_rate",
    "process",
    "process_block",
    "__advance_one_frame",
    "get_stream_output",
    "clear_event_outputs",
    "process_event_inputs",
    "tick_ramps",
    "latency_samples",
    "param_descriptors",
    "set_param",
    "set_param_immediate",
    "get_param",
];

/// Struct fields codegen always emits with fixed names.
const FIXED_FIELDS: &[&str] = &["sample_rate", "active_ramps"];

impl<'a> CodegenContext<'a> {
    /// Reject declarations whose (derived) generated names collide with the
    /// graph's built-in API or with names derived from another declaration.
    /// All collisions are reported, combined into one error.
    pub(super) fn check_generated_name_collisions(&self) -> syn::Result<()> {
        // Namespace maps: generated name -> what claimed it.
        let mut methods: HashMap<String, String> = HashMap::new();
        let mut fields: HashMap<String, String> = HashMap::new();
        for &m in FIXED_METHODS {
            methods.insert(m.to_string(), format!("the graph's built-in `{m}` method"));
        }
        for &f in FIXED_FIELDS {
            fields.insert(f.to_string(), format!("the graph's built-in `{f}` field"));
        }

        fn push_err(acc: &mut Option<syn::Error>, err: syn::Error) {
            match acc.as_mut() {
                Some(a) => a.combine(err),
                None => *acc = Some(err),
            }
        }
        fn claim(
            acc: &mut Option<syn::Error>,
            map: &mut HashMap<String, String>,
            generated: String,
            owner: String,
            decl: &syn::Ident,
            what: &str,
        ) {
            match map.entry(generated) {
                Entry::Occupied(prev) => {
                    push_err(
                        acc,
                        syn::Error::new(
                            decl.span(),
                            format!(
                                "{what} `{decl}` would generate `{}`, which collides with \
                                 {}; rename it",
                                prev.key(),
                                prev.get(),
                            ),
                        ),
                    );
                }
                Entry::Vacant(slot) => {
                    slot.insert(owner);
                }
            }
        }
        let mut acc: Option<syn::Error> = None;

        // Duplicate *field* names across declarations are already rejected
        // by lowering's duplicate-declaration check; here fields only need
        // checking against the fixed field set.
        for node in self
            .inputs()
            .chain(self.outputs())
            .chain(self.nodes())
        {
            let name = &node.name;
            if let Some(owner) = fields.get(&name.to_string()) {
                push_err(
                    &mut acc,
                    syn::Error::new(
                        name.span(),
                        format!("`{name}` collides with {owner}; rename it"),
                    ),
                );
            }
        }

        for node in self.inputs() {
            let name = &node.name;
            let ns = name.to_string();
            let ns = ns.strip_prefix("r#").unwrap_or(&ns).to_owned();
            match self.input_kind(name) {
                Some(EndpointKind::Value) => {
                    let owner = format!("the `set_{ns}` generated for value input `{name}`");
                    claim(&mut acc, &mut methods, format!("set_{ns}"), owner, name, "value input");
                    if self.is_ramped_input(name).is_some() {
                        for suffix in ["_with_ramp", "_immediate"] {
                            let owner = format!(
                                "the `set_{ns}{suffix}` generated for ramped input `{name}`"
                            );
                            claim(&mut acc, &mut methods,
                                format!("set_{ns}{suffix}"),
                                owner,
                                name,
                                "value input",
                            );
                        }
                    }
                }
                Some(EndpointKind::Event) => {
                    for (prefix, suffix) in [("push_", ""), ("handle_", "_events")] {
                        let owner = format!(
                            "the `{prefix}{ns}{suffix}` generated for event input `{name}`"
                        );
                        claim(&mut acc, &mut methods,
                            format!("{prefix}{ns}{suffix}"),
                            owner,
                            name,
                            "event input",
                        );
                    }
                }
                Some(EndpointKind::Stream) => {
                    let owner = format!("the `{ns}_block` generated for stream input `{name}`");
                    claim(&mut acc, &mut methods, format!("{ns}_block"), owner, name, "stream input");
                }
                _ => {}
            }
        }
        for node in self.outputs() {
            let name = &node.name;
            let ns = name.to_string();
            let ns = ns.strip_prefix("r#").unwrap_or(&ns).to_owned();
            if matches!(self.output_kind(name), Some(EndpointKind::Stream)) {
                let owner = format!("the `{ns}_block` generated for stream output `{name}`");
                claim(&mut acc, &mut methods, format!("{ns}_block"), owner, name, "stream output");
            }
        }

        match acc {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}
