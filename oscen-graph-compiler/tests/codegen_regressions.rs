//! Regression tests for codegen correctness fixes. These compile a graph and
//! assert on the generated token stream (or on a specific generated method's
//! body, extracted via `syn`).

use oscen_graph_compiler::compile;
use quote::{quote, ToTokens};

/// Compile a graph and return the generated tokens as a string.
fn compile_to_string(tokens: proc_macro2::TokenStream) -> String {
    match compile(tokens) {
        Ok(ts) => ts.to_string(),
        Err(diags) => panic!(
            "compile failed: {:?}",
            diags
                .items
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
        ),
    }
}

/// Extract the body of an inherent method from the generated code.
fn inherent_method_body(tokens: proc_macro2::TokenStream, method: &str) -> String {
    let file: syn::File = syn::parse2(tokens).expect("generated code parses as a file");
    for item in file.items {
        if let syn::Item::Impl(imp) = item {
            if imp.trait_.is_some() {
                continue;
            }
            for it in imp.items {
                if let syn::ImplItem::Fn(f) = it {
                    if f.sig.ident == method {
                        return f.block.to_token_stream().to_string();
                    }
                }
            }
        }
    }
    panic!("inherent method `{}` not found in generated code", method);
}

// ---------------------------------------------------------------------------
// Literal-left compound source (`0.5 * g.output -> out`)
// ---------------------------------------------------------------------------

#[test]
fn literal_left_compound_source_keeps_node_alive() {
    // Used to panic in debug (IR validator) and silently prune `g` in
    // release, because `primary_node` only descended the left operand.
    let tokens = compile(quote! {
        name: LitLeftCode;
        input stream s;
        output stream out;
        node g = Gain::new(0.5);
        connections {
            s -> g.input;
            0.5 * g.output -> out;
        }
    })
    .expect("compile succeeds")
    .to_string();
    assert!(
        tokens.contains("pub g :"),
        "node `g` must not be pruned; got no `g` field"
    );
    assert!(
        tokens.contains("self . g . output"),
        "output assignment should read g.output"
    );
}

// ---------------------------------------------------------------------------
// Dead-node pass and asset bindings
// ---------------------------------------------------------------------------

#[test]
fn asset_bound_node_survives_dead_node_pass() {
    // `player` has no path to a graph output; removing it used to leave the
    // AssetBinding pointing at a freed key, panicking in codegen
    // ("invalid SlotMap key used").
    let tokens = compile_to_string(quote! {
        name: AssetKeep;
        input stream s;
        output stream out;
        external sample: AudioAsset;
        node player = SamplePlayer::new();
        node g = Gain::new(0.5);
        connections {
            sample -> player.buf;
            s -> g.input;
            g.output -> out;
        }
    });
    assert!(
        tokens.contains("pub player :"),
        "asset-bound node `player` must stay alive"
    );
    assert!(
        tokens.contains("pub sample :"),
        "asset load handle field `sample` must be generated"
    );
}
