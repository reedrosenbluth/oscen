//! Integration tests for `ir::lower::lower`.

use oscen_graph_compiler::diagnostics::Diagnostics;
use oscen_graph_compiler::ir;
use oscen_graph_compiler::parse;
use quote::quote;

fn lower_quote(tokens: proc_macro2::TokenStream) -> (Option<ir::IrGraph>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let graph_def = parse::parse_graph_def(tokens, &mut diags);
    if !diags.is_empty() {
        return (None, diags);
    }
    let ir = ir::lower::lower(graph_def, &mut diags);
    (ir, diags)
}

#[test]
fn minimal_graph_lowers_to_input_and_output_nodes() {
    let (ir, diags) = lower_quote(quote! {
        name: Minimal;
        input stream s;
        output stream out;
    });
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}",
        diags.items.iter().map(|d| d.message.to_string()).collect::<Vec<_>>());
    let ir = ir.expect("lower should produce an IrGraph");

    assert_eq!(ir.name.to_string(), "Minimal");
    assert!(!ir.nih_params);
    assert_eq!(ir.inputs.len(), 1);
    assert_eq!(ir.outputs.len(), 1);
    assert_eq!(ir.processors.len(), 0);
    assert_eq!(ir.nodes.len(), 2);
}

#[test]
fn duplicate_declaration_accumulates_error() {
    let (ir, diags) = lower_quote(quote! {
        name: Dup;
        input stream s;
        input stream s;
    });
    assert!(ir.is_none(), "lower should return None on duplicate");
    let msgs: Vec<String> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        msgs.iter().any(|m| m.contains("duplicate declaration")),
        "expected duplicate-declaration error; got: {msgs:?}"
    );
}
