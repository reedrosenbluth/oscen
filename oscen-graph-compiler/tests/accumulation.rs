use oscen_graph_compiler::compile;
use quote::quote;

#[test]
fn compile_accumulates_two_type_mismatches() {
    let input = quote! {
        name: TwoMismatches;
        input stream s1;
        input stream s2;
        output value v_out;
        connections {
            s1 -> v_out;
            s2 -> v_out;
        }
    };

    let result = compile(input);
    let diags = result.expect_err("expected diagnostics; got Ok");
    let errors: Vec<_> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .collect();
    assert_eq!(
        errors.len(),
        2,
        "expected two type-mismatch errors, got {}: {:?}",
        errors.len(),
        errors
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
    );
    for e in &errors {
        assert!(
            e.message.to_string().contains("Type mismatch in connection"),
            "unexpected error: {}",
            e.message
        );
    }
}

#[test]
fn compile_accumulates_rate_and_type_errors() {
    let input = quote! {
        name: MixedErrors;
        input stream s;
        output value v_out;
        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6) / 2;
        }
        connections {
            s -> v_out;
        }
    };

    let result = compile(input);
    let diags = result.expect_err("expected diagnostics; got Ok");
    let messages: Vec<String> = diags
        .items
        .iter()
        .map(|d| d.message.to_string())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("undersampling")),
        "expected an undersampling error; got: {:?}",
        messages
    );
    assert!(
        messages.iter().any(|m| m.contains("Type mismatch in connection")),
        "expected a type-mismatch error; got: {:?}",
        messages
    );
}

#[test]
fn compile_ok_for_valid_graph_returns_no_diagnostics() {
    let input = quote! {
        name: Valid;
        input stream s;
        output stream out;
        connections {
            s -> out;
        }
    };

    let result = compile(input);
    assert!(result.is_ok(), "expected Ok; got Err: {:?}", result.err());
}
