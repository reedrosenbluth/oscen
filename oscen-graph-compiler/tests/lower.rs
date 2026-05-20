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
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags
            .items
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
    );
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
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags.items
    );
    let ir = ir.expect("lower should produce an IrGraph");

    assert_eq!(ir.edges.len(), 1);
    let edge = ir.edges.values().next().unwrap();
    let src_ep = match &edge.source.kind {
        oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) => ep,
        _ => panic!("expected Endpoint source"),
    };
    let s_node = &ir.nodes[src_ep.node];
    let out_node = &ir.nodes[edge.dest.node];
    assert_eq!(s_node.name.to_string(), "s");
    assert_eq!(out_node.name.to_string(), "out");

    // Inputs always have endpoints populated by collect_declarations.
    assert_eq!(
        s_node.endpoints[&src_ep.endpoint].kind,
        oscen_graph_compiler::ast::EndpointKind::Stream
    );
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
    let errors: Vec<_> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .collect();
    assert_eq!(
        errors.len(),
        2,
        "expected two type-mismatch errors, got: {:?}",
        errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn upsampled_node_carries_rate_factor() {
    let (ir, diags) = lower_quote(quote! {
        name: Up;
        input stream s;
        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.5) * 4;
        connections {
            s -> osc.frequency;
            osc.output -> out;
        }
    });
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags.items
    );
    let ir = ir.expect("lower should produce an IrGraph");

    let osc = ir
        .processors
        .iter()
        .find_map(|&id| (ir.nodes[id].name == "osc").then(|| &ir.nodes[id]))
        .expect("osc node should exist");
    assert!(
        matches!(osc.rate, oscen_graph_compiler::ast::NodeRate::Up(4)),
        "expected NodeRate::Up(4), got {:?}",
        osc.rate
    );
}

#[test]
fn cross_rate_edge_picks_correct_kernel() {
    let (ir, diags) = lower_quote(quote! {
        name: CrossRate;
        input stream s;
        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.5) * 4;
        connections {
            s -> osc.frequency;
            osc.output -> out;
        }
    });
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags.items
    );
    let ir = ir.expect("lower should produce an IrGraph");

    // The edge `osc.output -> out` crosses from rate x4 to rate x1 (graph rate)
    // and should have a non-None kernel (i.e., something other than EdgeKernel::None).
    let cross_edges: Vec<_> = ir
        .edges
        .values()
        .filter(|e| {
            let src_name = if let oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) = &e.source.kind {
                ir.nodes[ep.node].name.to_string()
            } else {
                String::new()
            };
            src_name == "osc" && ir.nodes[e.dest.node].name == "out"
        })
        .collect();
    assert_eq!(cross_edges.len(), 1);
    let kernel = &cross_edges[0].kernel;
    assert!(
        !matches!(kernel, oscen_graph_compiler::ir::EdgeKernel::None),
        "expected a non-None (cross-rate) kernel, got {:?}",
        kernel
    );
}

#[test]
fn scalar_edges_get_scalar_fanout() {
    let (ir, diags) = lower_quote(quote! {
        name: Scalar;
        input stream s;
        output stream out;
        connections { s -> out; }
    });
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags.items
    );
    let ir = ir.expect("lower should produce an IrGraph");

    assert!(!ir.edges.is_empty(), "expected at least one edge");
    for edge in ir.edges.values() {
        assert!(
            matches!(edge.fanout, oscen_graph_compiler::ir::FanoutShape::Scalar),
            "expected FanoutShape::Scalar, got {:?}",
            edge.fanout
        );
    }
}

#[test]
fn topo_sort_orders_branching_graph() {
    let (ir, diags) = lower_quote(quote! {
        name: Branch;
        input stream s;
        output stream out;
        // b is declared before a, but topologically depends on a.
        // Without topo_sort, b would appear before a in processors[].
        node b = Gain::new(0.5);
        node a = Gain::new(0.5);
        connections {
            s -> a.input;
            a.output -> b.input;
            b.output -> out;
        }
    });
    assert!(diags.is_empty(), "{:?}", diags.items);
    let ir = ir.expect("lower should produce an IrGraph");

    let a_pos = ir
        .processors
        .iter()
        .position(|&id| ir.nodes[id].name == "a")
        .unwrap();
    let b_pos = ir
        .processors
        .iter()
        .position(|&id| ir.nodes[id].name == "b")
        .unwrap();
    assert!(
        a_pos < b_pos,
        "a (upstream of b) should come first in topo order, got a={a_pos} b={b_pos}"
    );
}

#[test]
fn non_feedback_cycle_with_extra_delay_input_is_rejected() {
    // X <-> Y is a non-feedback cycle that must be rejected.
    // A `#[feedback]`-marked node feeding into X must NOT mask that cycle.
    let (ir, diags) = lower_quote(quote! {
        name: BadCycle;
        input stream src;
        output stream out;
        node x = Gain::new(0.5);
        node y = Gain::new(0.5);
        #[feedback]
        node d = MyDelay::new(0.1);
        connections {
            src -> x.input;
            x.output -> y.input;
            y.output -> x.input;
            d.output -> x.input;
            x.output -> out;
        }
    });
    // We expect cycle detection to fire because X <-> Y is a non-feedback cycle.
    // The Delay's edge to x shouldn't mask it.
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    assert!(
        ir.is_none()
            && errors
                .iter()
                .any(|e| e.contains("cycle") || e.contains("Cycle")),
        "expected cycle detection; got ir.is_some()={} errors={:?}",
        ir.is_some(),
        errors
    );
}

#[test]
fn feedback_attr_allows_cycle_on_non_delay_named_node() {
    // `#[feedback]` on a node whose type name has nothing to do with "Delay"
    // breaks a cycle for topological sort. Proves feedback opt-in works via
    // the explicit attribute, not via substring matching on the type name.
    let (ir, diags) = lower_quote(quote! {
        name: FbOk;
        input stream src;
        output stream out;
        node g = Gain::new(0.5);
        #[feedback]
        node ring = RingBuffer::new(64);
        connections {
            src -> g.input;
            g.output -> ring.input;
            ring.output -> g.input;
            g.output -> out;
        }
    });
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    assert!(
        ir.is_some() && errors.is_empty(),
        "expected feedback-allowed cycle to lower cleanly; got errors={:?}",
        errors,
    );
}

#[test]
fn delay_named_node_without_feedback_attr_is_a_cycle() {
    // A node whose type name is literally `Delay` but has no `#[feedback]`
    // attribute does NOT participate in feedback. Proves the old substring
    // heuristic (`name.contains("Delay")`) is gone — only the attribute counts.
    let (ir, diags) = lower_quote(quote! {
        name: DelayNoAttr;
        input stream src;
        output stream out;
        node g = Gain::new(0.5);
        node d = Delay::new(0.1);
        connections {
            src -> g.input;
            g.output -> d.input;
            d.output -> g.input;
            g.output -> out;
        }
    });
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    assert!(
        ir.is_none()
            && errors
                .iter()
                .any(|e| e.contains("cycle") || e.contains("Cycle")),
        "expected cycle detection on an unannotated `Delay`; got ir.is_some()={} errors={:?}",
        ir.is_some(),
        errors,
    );
}

#[test]
fn validate_cross_rate_kinds_smoke() {
    // A well-formed multi-rate graph passes validation cleanly.
    let (ir, diags) = lower_quote(quote! {
        name: Smoke;
        input stream s;
        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.5) * 4;
        connections {
            s -> osc.frequency;
            osc.output -> out;
        }
    });
    assert!(
        diags.is_empty(),
        "unexpected diagnostics: {:?}",
        diags.items
    );
    assert!(ir.is_some(), "expected lower to produce an IrGraph");
}

#[test]
fn unconnected_down_node_produces_undersampling_error() {
    let (ir, diags) = lower_quote(quote! {
        name: BadRate;
        input stream s;
        output stream out;
        node x = Gain::new(0.5) / 2;
        connections {
            s -> out;
        }
    });
    assert!(ir.is_none(), "lower should reject undersampling");
    let msgs: Vec<String> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        msgs.iter()
            .any(|m| m.to_lowercase().contains("undersampling")),
        "expected undersampling error; got: {:?}",
        msgs
    );
}
