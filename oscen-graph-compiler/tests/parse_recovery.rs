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
            s1 => out;
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

#[test]
fn inline_delay_literal_parses_as_samples_via() {
    use oscen_graph_compiler::ast::{DelayVia, GraphItem};
    use oscen_graph_compiler::Diagnostics;
    let toks: proc_macro2::TokenStream = "name: G; node a = Foo::new(); node b = Foo::new(); connections { a.output -> [4] -> b.input; }"
        .parse().unwrap();
    let mut diags = Diagnostics::new();
    let parsed = oscen_graph_compiler::parse::parse_graph_def(toks, &mut diags);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
    let block = parsed
        .items
        .iter()
        .find_map(|item| match item {
            GraphItem::ConnectionBlock(b) => Some(b),
            _ => None,
        })
        .expect("connection block");
    assert_eq!(block.0.len(), 1);
    let via = block.0[0].via.as_ref().expect("via on inline-delay edge");
    match via {
        DelayVia::Samples { value, .. } => {
            assert_eq!(value.base10_parse::<u32>().unwrap(), 4);
        }
        _ => panic!("expected Samples variant"),
    }
}

#[test]
fn inline_delay_ident_parses_as_node_via() {
    use oscen_graph_compiler::ast::{DelayVia, GraphItem};
    use oscen_graph_compiler::Diagnostics;
    let toks: proc_macro2::TokenStream = "name: G; node a = Foo::new(); node d = Delay::new(0.0, 0.0); node b = Foo::new(); connections { a.output -> [d] -> b.input; }"
        .parse().unwrap();
    let mut diags = Diagnostics::new();
    let parsed = oscen_graph_compiler::parse::parse_graph_def(toks, &mut diags);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
    let block = parsed
        .items
        .iter()
        .find_map(|item| match item {
            GraphItem::ConnectionBlock(b) => Some(b),
            _ => None,
        })
        .unwrap();
    let via = block.0[0].via.as_ref().expect("via");
    match via {
        DelayVia::Node { name } => assert_eq!(name.to_string(), "d"),
        _ => panic!("expected Node variant"),
    }
}

#[test]
fn tilde_arrow_no_longer_parses() {
    use oscen_graph_compiler::Diagnostics;
    let toks: proc_macro2::TokenStream =
        "name: G; node a = Foo::new(); node b = Foo::new(); connections { a.output ~> b.input; }"
            .parse()
            .unwrap();
    let mut diags = Diagnostics::new();
    let _parsed = oscen_graph_compiler::parse::parse_graph_def(toks, &mut diags);
    assert!(!diags.is_empty(), "expected parse error for ~>");
    let msg = diags
        .items
        .iter()
        .map(|d| d.message.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !msg.contains("~>"),
        "diagnostic should not advertise ~>; got: {}",
        msg
    );
}
