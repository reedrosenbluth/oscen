//! Compiler for the Oscen `graph!` DSL.
//!
//! This crate is consumed by `oscen-macros` (proc-macro shim) today and
//! is designed so future tooling (build.rs, LSP) can consume it directly.

pub mod ast;
pub mod codegen;
pub mod fanout;
pub mod parse;
pub mod rate_analysis;
pub mod type_check;

/// Compile a `graph!` body into the generated graph struct + impls.
///
/// Returns the generated tokens on success, or a `syn::Error` describing
/// the first failure. (Phase 2a wraps the existing single-error path; the
/// `Diagnostics` boundary type is added in Task 4.)
pub fn compile(
    input: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let graph_def: ast::GraphDef = syn::parse2(input)?;
    codegen::generate(&graph_def)
}
