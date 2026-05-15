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
/// diagnostics on failure. Type-mismatch and rate-analysis errors are
/// accumulated across all connections (and reported in a single compile
/// cycle); parse errors and codegen errors continue to surface a single
/// `syn::Error` wrapped in a one-element `Diagnostics`.
pub fn compile(
    input: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, Diagnostics> {
    let graph_def: ast::GraphDef = syn::parse2(input).map_err(Diagnostics::from)?;
    codegen::generate(&graph_def)
}
