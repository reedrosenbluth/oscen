use proc_macro::TokenStream;
// Stand-in for oscen's real "finish graph codegen" proc macro: counts the
// tokens it received from the manifest chain and emits a const.
#[proc_macro]
pub fn finish(input: TokenStream) -> TokenStream {
    let n = input.into_iter().count();
    format!("pub const TOKENS_SEEN: usize = {n};").parse().unwrap()
}
