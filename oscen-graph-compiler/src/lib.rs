//! Compiler for the Oscen `graph!` DSL.
//!
//! This crate is consumed by `oscen-macros` (proc-macro shim) today and
//! is designed so future tooling (build.rs, LSP) can consume it directly.

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod fanout;
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
/// single compile cycle. Codegen errors still surface a single
/// `syn::Error` wrapped in a one-element `Diagnostics`. If any parse
/// error occurs, the validation passes are skipped to avoid emitting
/// misleading errors on a partial AST.
pub fn compile(input: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, Diagnostics> {
    let mut diags = Diagnostics::new();
    let graph_def = parse::parse_graph_def(input, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    codegen::generate(&graph_def)
}
