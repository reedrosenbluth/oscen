//! Compiler for the Oscen `graph!` DSL.
//!
//! This crate is consumed by `oscen-macros` (proc-macro shim) today and
//! is designed so future tooling (build.rs, LSP) can consume it directly.

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod ir;
pub mod manifest;
pub mod parse;

pub use diagnostics::{Diagnostic, Diagnostics, Severity};

/// Compile a `graph!` body into the generated graph struct + impls.
///
/// Returns the generated tokens on success; returns the accumulated
/// diagnostics on failure. Parse errors are accumulated across
/// independent top-level items and across statements inside
/// `node {}` / `connection {}` blocks. Type-mismatch and
/// rate-analysis errors are accumulated across all connections in a
/// single compile cycle.
///
/// Wildcard hoists (`input node.*;`) require resolved endpoint manifests;
/// use [`compile_with_manifests`] (the `graph!` proc macro's two-stage
/// expansion collects them). Calling `compile` on a body with wildcards
/// reports a "no endpoint manifest resolved" diagnostic per wildcard.
pub fn compile(input: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, Diagnostics> {
    compile_with_manifests(input, &std::collections::HashMap::new())
}

/// Compile a `graph!` body whose wildcard hoists (`input node.*;`) have
/// their endpoint manifests resolved in `manifests` (keyed by node name).
///
/// This is the re-entry point for the two-stage wildcard expansion: the
/// `graph!` proc macro detects wildcards with [`manifest::scan_wildcards`],
/// chains the child types' manifest macros in continuation-passing style,
/// and the final continuation calls this with the collected manifests.
pub fn compile_with_manifests(
    input: proc_macro2::TokenStream,
    manifests: &std::collections::HashMap<String, manifest::NodeManifest>,
) -> Result<proc_macro2::TokenStream, Diagnostics> {
    let mut diags = Diagnostics::new();
    let mut graph_def = parse::parse_graph_def(input, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    manifest::expand_wildcards(&mut graph_def, manifests, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    let mut ir = match ir::lower::lower(graph_def, &mut diags) {
        Some(ir) => ir,
        None => return Err(diags),
    };
    ir::passes::dead_nodes::run(&mut ir);
    codegen::generate(&ir)
}
