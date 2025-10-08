mod ast;
mod parse;
mod codegen;
mod type_check;

use proc_macro::TokenStream;
use syn::parse_macro_input;

pub fn graph_impl(input: TokenStream) -> TokenStream {
    let graph_def = parse_macro_input!(input as ast::GraphDef);

    match codegen::generate(&graph_def) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
