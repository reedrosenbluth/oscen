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

#[test]
fn linear_chain_lowers_with_typed_edges() {
    let (ir, diags) = lower_quote(quote! {
        name: Linear;
        input stream s;
        output stream out;
        connections {
            s -> out;
        }
    });
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags.items);
    let ir = ir.expect("lower should produce an IrGraph");

    assert_eq!(ir.edges.len(), 1);
    let edge = ir.edges.values().next().unwrap();
    let s_node = &ir.nodes[edge.source.node];
    let out_node = &ir.nodes[edge.dest.node];
    assert_eq!(s_node.name.to_string(), "s");
    assert_eq!(out_node.name.to_string(), "out");

    // Inputs always have endpoints populated by collect_declarations.
    assert_eq!(s_node.endpoints[&edge.source.endpoint].kind,
               oscen_graph_compiler::ast::EndpointKind::Stream);
}

#[test]
fn type_mismatch_accumulates_per_connection() {
    let (ir, diags) = lower_quote(quote! {
        name: Mismatch;
        input stream s1;
        input stream s2;
        output value v_out;
        connections {
            s1 -> v_out;
            s2 -> v_out;
        }
    });
    assert!(ir.is_none(), "lower should return None on type errors");
    let errors: Vec<_> = diags.items.iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .collect();
    assert_eq!(errors.len(), 2, "expected two type-mismatch errors, got: {:?}",
        errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>());
}
