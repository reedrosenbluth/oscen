//! Compiler for the Oscen `graph!` DSL.
//!
//! This crate is consumed by `oscen-macros` (proc-macro shim) today and
//! is designed so future tooling (build.rs, LSP) can consume it directly.

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod fanout;
pub mod ir;
pub mod parse;
pub mod rate_analysis;
pub mod type_check;

pub use diagnostics::{Diagnostic, Diagnostics, Severity};

/// Compile a `graph!` body into the generated graph struct + impls.
///
/// Returns the generated tokens on success; returns the accumulated
/// diagnostics on failure. Parse errors are accumulated across
/// independent top-level items and across statements inside
/// `node {}` / `connection {}` blocks. Type-mismatch and
/// rate-analysis errors are accumulated across all connections in a
/// single compile cycle.
pub fn compile(input: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, Diagnostics> {
    let mut diags = Diagnostics::new();
    let graph_def = parse::parse_graph_def(input, &mut diags);
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
