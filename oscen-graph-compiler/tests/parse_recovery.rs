use oscen_graph_compiler::{compile, Diagnostics, Severity};
use quote::quote;

/// Helper: count `Error` diagnostics.
fn error_count(diags: &Diagnostics) -> usize {
    diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .count()
}

#[test]
fn accumulates_two_top_level_parse_errors() {
    let input = quote! {
        name: TwoBadItems;
        input stream s1
        input stream s2;
        output stream out;
        foo bar baz;
        connections {
            s1 -> out;
        }
    };

    let diags = compile(input).expect_err("expected diagnostics; got Ok");
    assert_eq!(
        error_count(&diags),
        2,
        "expected two parse errors; got: {:?}",
        diags
            .items
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn accumulates_two_parse_errors_in_node_block() {
    let input = quote! {
        name: BadNodeBlock;
        input stream s;
        output stream out;
        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.6)
            lfo = PolyBlepOscillator::sine(2.0, 0.5);
            amp $ 0.8;
        }
        connections {
            s -> out;
        }
    };

    let diags = compile(input).expect_err("expected diagnostics; got Ok");
    assert_eq!(
        error_count(&diags),
        2,
        "expected two parse errors; got: {:?}",
        diags
            .items
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn accumulates_two_parse_errors_in_connection_block() {
    let input = quote! {
        name: BadConnectionBlock;
        input stream s1;
        input stream s2;
        input stream s3;
        output stream out;
        connections {
            s1 ~> out;
            s2 -> out;
            s3 -> ;
        }
    };

    let diags = compile(input).expect_err("expected diagnostics; got Ok");
    assert_eq!(
        error_count(&diags),
        2,
        "expected two parse errors; got: {:?}",
        diags
            .items
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn misplaced_name_decl_produces_ordering_error() {
    let input = quote! {
        input stream s;
        name: LateName;
        output stream out;
        connections {
            s -> out;
        }
    };

    let diags = compile(input).expect_err("expected diagnostics; got Ok");
    let messages: Vec<String> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("must appear at the start")),
        "expected ordering error; got: {:?}",
        messages
    );
}

#[test]
fn valid_graph_produces_no_diagnostics() {
    let input = quote! {
        name: Valid;
        input stream s;
        output stream out;
        connections {
            s -> out;
        }
    };
    assert!(compile(input).is_ok(), "expected Ok for valid graph");
}
