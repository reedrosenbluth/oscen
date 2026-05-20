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
            let src_name =
                if let oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) = &e.source.kind {
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
    // A `Delay` feeding into X (but not part of the X<->Y cycle) must NOT
    // mask the cycle.
    let (ir, diags) = lower_quote(quote! {
        name: BadCycle;
        input stream src;
        output stream out;
        node x = Gain::new(0.5);
        node y = Gain::new(0.5);
        node d = Delay::new(0.1);
        connections {
            src -> x.input;
            x.output -> y.input;
            y.output -> x.input;
            d.output -> x.input;
            x.output -> out;
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
        "expected cycle detection; got ir.is_some()={} errors={:?}",
        ir.is_some(),
        errors
    );
}

#[test]
fn feedback_arrow_breaks_cycle() {
    // A cycle closed by a `-> [1] ->` edge lowers successfully — topo sort
    // skips the synth-delay's outgoing leg. Choice of node type at the source
    // is irrelevant; this
    // test deliberately uses `Gain` (no AllowsFeedback impl) because that
    // check is enforced at codegen time, not during lowering.
    let (ir, diags) = lower_quote(quote! {
        name: ArrowCycle;
        input stream src;
        output stream out;
        node g = Gain::new(0.5);
        node h = Gain::new(0.5);
        connections {
            src -> g.input;
            g.output -> h.input;
            h.output -> [1] -> g.input;
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
        "expected inline-delay-broken cycle to lower cleanly; got errors={:?}",
        errors,
    );
}

#[test]
fn plain_arrow_cycle_is_rejected() {
    // A cycle closed only by `->` edges is a non-feedback cycle. The
    // identity / type of the nodes doesn't matter — even using a node named
    // `Delay`, without `-> [...] ->` syntax the cycle is rejected.
    let (ir, diags) = lower_quote(quote! {
        name: PlainArrowCycle;
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
        "expected cycle detection on `->`-only cycle; got ir.is_some()={} errors={:?}",
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

#[test]
fn feedback_arrow_lowers_with_is_feedback_set() {
    // Sanity check: the parser/lower pipeline carries the inline-delay flag
    // onto the resulting IrEdge.
    let (ir, diags) = lower_quote(quote! {
        name: ArrowFlag;
        input stream src;
        output stream out;
        node g = Gain::new(0.5);
        node d = Delay::new(0.1);
        connections {
            src -> g.input;
            g.output -> d.input;
            d.output -> [1] -> g.input;
            g.output -> out;
        }
    });
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    let ir = ir.unwrap_or_else(|| panic!("expected lower to succeed; errors={:?}", errors));
    let feedback_edges = ir.edges.values().filter(|e| e.is_feedback).count();
    assert_eq!(
        feedback_edges, 1,
        "expected exactly one feedback edge from inline-delay, got {}",
        feedback_edges,
    );
}

#[test]
fn inline_node_via_creates_two_edges_through_declared_delay() {
    let (ir, diags) = lower_quote(quote! {
        name: G;
        node a = oscen::Gain::new(1.0);
        node d = oscen::Delay::new(11025.0, 0.0);
        node b = oscen::Gain::new(1.0);
        connections {
            a.output -> b.input;
            b.output -> [d] -> a.input;
        }
    });
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    let ir = ir.unwrap_or_else(|| panic!("lower failed: {:?}", errors));

    let d_id = ir
        .nodes
        .iter()
        .find(|(_, n)| n.name == "d")
        .map(|(id, _)| id)
        .expect("node d");

    assert_eq!(
        ir.edges.len(),
        3,
        "expected 3 edges; got {}",
        ir.edges.len()
    );

    let through_d_in = ir.edges.values().filter(|e| e.dest.node == d_id).count();
    let through_d_out = ir.edges.values().filter(|e| {
        matches!(&e.source.kind, oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) if ep.node == d_id)
    }).count();
    assert_eq!(through_d_in, 1, "exactly one edge should enter d.input");
    assert_eq!(through_d_out, 1, "exactly one edge should leave d.output");

    let outgoing_is_feedback = ir.edges.values().any(|e| {
        e.is_feedback
            && matches!(&e.source.kind, oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) if ep.node == d_id)
    });
    assert!(
        outgoing_is_feedback,
        "the d.output -> a.input edge should be marked is_feedback"
    );

    // The other two edges should NOT be marked is_feedback.
    let feedback_count = ir.edges.values().filter(|e| e.is_feedback).count();
    assert_eq!(feedback_count, 1, "exactly one feedback edge total");
}

#[test]
fn inline_literal_via_synthesizes_delay_node() {
    let (ir, diags) = lower_quote(quote! {
        name: G;
        node a = oscen::Gain::new(1.0);
        node b = oscen::Gain::new(1.0);
        connections {
            a.output -> b.input;
            b.output -> [128] -> a.input;
        }
    });
    let errors: Vec<String> = diags
        .items
        .iter()
        .filter(|d| matches!(d.severity, oscen_graph_compiler::Severity::Error))
        .map(|d| d.message.to_string())
        .collect();
    let ir = ir.unwrap_or_else(|| panic!("lower failed: {:?}", errors));

    // Exactly one synthetic Delay should be inserted. Detect by name prefix.
    let synth_count = ir
        .nodes
        .iter()
        .filter(|(_, n)| n.name.to_string().starts_with("__inline_delay_"))
        .count();
    assert_eq!(
        synth_count, 1,
        "expected one synthetic delay; got {}",
        synth_count
    );

    let (synth_id, synth_node) = ir
        .nodes
        .iter()
        .find(|(_, n)| n.name.to_string().starts_with("__inline_delay_"))
        .expect("synth delay node");
    match &synth_node.kind {
        oscen_graph_compiler::ir::graph::IrNodeKind::Processor { ctor_expr, .. } => {
            let ctor_str = quote!(#ctor_expr).to_string();
            assert!(
                ctor_str.contains("Delay") && ctor_str.contains("128"),
                "expected ctor referencing Delay and 128 samples; got `{}`",
                ctor_str
            );
        }
        _ => panic!("synth node should be Processor kind"),
    }

    // Total edges: a->b, b->synth (normal), synth->a (feedback) = 3.
    assert_eq!(
        ir.edges.len(),
        3,
        "expected 3 edges; got {}",
        ir.edges.len()
    );
    let feedback_count = ir.edges.values().filter(|e| e.is_feedback).count();
    assert_eq!(feedback_count, 1);

    let fb_edge = ir.edges.values().find(|e| e.is_feedback).unwrap();
    match &fb_edge.source.kind {
        oscen_graph_compiler::ir::IrExprKind::Endpoint(ep) => {
            assert_eq!(
                ep.node, synth_id,
                "feedback edge should leave the synth node"
            );
            assert_eq!(ep.endpoint.to_string(), "output");
        }
        _ => panic!("feedback edge source should be a simple Endpoint"),
    }
}

#[test]
fn inline_node_via_undeclared_errors() {
    let (ir, diags) = lower_quote(quote! {
        name: G;
        node a = oscen::Gain::new(1.0);
        node b = oscen::Gain::new(1.0);
        connections {
            a.output -> b.input;
            b.output -> [nonexistent] -> a.input;
        }
    });
    let msgs: Vec<_> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        ir.is_none(),
        "expected lower to fail on undeclared via node; got Some(ir)"
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("unknown node") && m.contains("nonexistent")),
        "expected `unknown node ... nonexistent` error; got: {:?}",
        msgs
    );
}

#[test]
fn inline_node_via_double_use_errors() {
    let (ir, diags) = lower_quote(quote! {
        name: G;
        node a = oscen::Gain::new(1.0);
        node b = oscen::Gain::new(1.0);
        node c = oscen::Gain::new(1.0);
        node d = oscen::Delay::new(0.0, 0.0);
        connections {
            a.output -> [d] -> b.input;
            b.output -> [d] -> c.input;
        }
    });
    let msgs: Vec<_> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        ir.is_none(),
        "expected lower to fail on double-use of via node; got Some(ir)"
    );
    assert!(
        msgs.iter().any(|m| m.contains("already wired")),
        "expected `already wired` error; got: {:?}",
        msgs
    );
}

#[test]
fn plain_arrow_cycle_diagnostic_mentions_bracket_syntax() {
    let (ir, diags) = lower_quote(quote! {
        name: G;
        node a = oscen::Gain::new(1.0);
        node b = oscen::Gain::new(1.0);
        connections {
            a.output -> b.input;
            b.output -> a.input;
        }
    });
    let msgs: Vec<_> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(
        ir.is_none(),
        "expected lower to fail on non-feedback cycle; got Some(ir)"
    );
    assert!(
        msgs.iter().any(|m| m.contains("-> [")),
        "expected cycle diagnostic to suggest bracket syntax; got: {:?}",
        msgs
    );
}
