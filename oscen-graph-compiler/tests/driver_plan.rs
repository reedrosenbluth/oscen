//! Tests for the resolved driver plan (`ir::passes::drivers`) and the
//! whole-expression clock (`ir::lower::expression_clock`).
//!
//! These pin the *semantic* decisions lowering makes about multi-driver
//! slots and mixed-rate expressions, independently of what codegen emits.

use oscen_graph_compiler::ast::{EndpointKind, NodeRate};
use oscen_graph_compiler::diagnostics::Diagnostics;
use oscen_graph_compiler::ir::{self, EdgeKernel, Element, FanInPolicy, KindEvidence};
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

fn lower_ok(tokens: proc_macro2::TokenStream) -> ir::IrGraph {
    let (ir, diags) = lower_quote(tokens);
    let msgs: Vec<String> = diags.items.iter().map(|d| d.message.to_string()).collect();
    assert!(msgs.is_empty(), "unexpected diagnostics: {msgs:?}");
    ir.expect("lower should succeed")
}

fn lower_errors(tokens: proc_macro2::TokenStream) -> Vec<String> {
    let (ir, diags) = lower_quote(tokens);
    assert!(ir.is_none(), "lowering should fail");
    diags.items.iter().map(|d| d.message.to_string()).collect()
}

fn node_id(ir: &ir::IrGraph, name: &str) -> ir::NodeId {
    ir.nodes
        .iter()
        .find(|(_, n)| n.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("node `{name}` not found"))
}

fn group_for<'a>(ir: &'a ir::IrGraph, node: &str, field: &str, index: Option<usize>) -> &'a ir::DriverGroup {
    let id = node_id(ir, node);
    ir.drivers
        .groups
        .iter()
        .find(|g| {
            g.dest.node == id
                && g.dest.field == field
                && match (g.dest.element, index) {
                    (Element::Index(k), Some(i)) => k == i,
                    (Element::Index(_), None) => false,
                    (_, None) => true,
                    (_, Some(_)) => false,
                }
        })
        .unwrap_or_else(|| panic!("no driver group for {node}.{field}[{index:?}]"))
}

// ---------------------------------------------------------------------------
// Grouping and policy
// ---------------------------------------------------------------------------

#[test]
fn every_edge_belongs_to_exactly_one_group_in_canonical_order() {
    let ir = lower_ok(quote! {
        name: G;
        input stream a;
        input stream b;
        output stream out;
        node g = Gain::new(1.0);
        connections {
            a -> g.input;
            b -> g.input;
            g.output -> out;
        }
    });
    assert_eq!(ir.drivers.groups.len(), 2);
    assert_eq!(ir.drivers.group_of_edge.len(), ir.edges.len());
    let g_in = group_for(&ir, "g", "input", None);
    assert_eq!(g_in.sources, ir.edge_order[..2].to_vec(), "sources keep edge order");
    assert_eq!(ir.drivers.groups[0].sources[0], ir.edge_order[0]);
    assert_eq!(ir.drivers.groups[1].sources[0], ir.edge_order[2]);
}

#[test]
fn known_stream_fan_in_is_sum() {
    let ir = lower_ok(quote! {
        name: G;
        input stream a;
        input stream b;
        output stream out;
        node g = Gain::new(1.0);
        connections {
            a -> g.input;
            b -> g.input;
            g.output -> out;
        }
    });
    let g = group_for(&ir, "g", "input", None);
    assert_eq!(g.kind, KindEvidence::Known(EndpointKind::Stream));
    assert_eq!(g.policy, FanInPolicy::Sum);
    assert!(!g.rustc_kind_check);
}

#[test]
fn known_event_fan_in_is_append() {
    let ir = lower_ok(quote! {
        name: G;
        input event a;
        input event b;
        node sink = Sink::new();
        connections {
            a -> sink.ev;
            b -> sink.ev;
        }
    });
    let g = group_for(&ir, "sink", "ev", None);
    assert_eq!(g.kind, KindEvidence::Known(EndpointKind::Event));
    assert_eq!(g.policy, FanInPolicy::Append);
    assert!(!g.rustc_kind_check);
}

#[test]
fn graph_event_output_fan_in_is_append() {
    let ir = lower_ok(quote! {
        name: G;
        output event o;
        nodes {
            a = Src::new();
            b = Src::new();
        }
        connections {
            a.ev -> o;
            b.ev -> o;
        }
    });
    let g = group_for(&ir, "o", "o", None);
    assert!(g.dest.bare);
    assert_eq!(g.policy, FanInPolicy::Append);
}

#[test]
fn single_driver_is_single_even_when_kind_unknown() {
    let ir = lower_ok(quote! {
        name: G;
        output stream out;
        nodes {
            a = Src::new();
            g = Gain::new(1.0);
        }
        connections {
            a.output -> g.input;
            g.output -> out;
        }
    });
    let g = group_for(&ir, "g", "input", None);
    assert_eq!(g.kind, KindEvidence::Unknown);
    assert_eq!(g.policy, FanInPolicy::Single);
    assert!(!g.rustc_kind_check);
}

#[test]
fn unknown_kind_fan_in_defers_to_rustc() {
    // Pure node-to-node: the IR cannot know whether `sink.input` is a stream
    // or a value. It gets the accumulate shape plus a rustc kind assertion,
    // never a silent "assume stream".
    let ir = lower_ok(quote! {
        name: G;
        output stream out;
        nodes {
            a = Src::new();
            b = Src::new();
            sink = Gain::new(1.0);
        }
        connections {
            a.output -> sink.input;
            b.output -> sink.input;
            sink.output -> out;
        }
    });
    let g = group_for(&ir, "sink", "input", None);
    assert_eq!(g.kind, KindEvidence::Unknown);
    assert_eq!(g.policy, FanInPolicy::Sum);
    assert!(g.rustc_kind_check, "unknown-kind multi-driver must be checked by rustc");
}

#[test]
fn unknown_kind_fan_in_into_untyped_node_is_rejected() {
    // A node built from an arbitrary expression has no type path to project
    // an `EndpointAt` marker from, so rustc cannot veto a value slot either.
    let msgs = lower_errors(quote! {
        name: G;
        output stream out;
        nodes {
            a = Src::new();
            b = Src::new();
            sink = make_sink();
        }
        connections {
            a.output -> sink.input;
            b.output -> sink.input;
            sink.output -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("cannot verify that `sink.input` accepts multiple drivers")),
        "got {msgs:?}"
    );
}

#[test]
fn known_value_fan_in_is_rejected() {
    let msgs = lower_errors(quote! {
        name: G;
        input value a;
        input value b;
        output stream out;
        node g = Gain::new(1.0);
        connections {
            a -> g.gain;
            b -> g.gain;
            g.output -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("value endpoint `g.gain` has 2 sources")),
        "got {msgs:?}"
    );
}

#[test]
fn cross_rate_event_fan_in_is_rejected() {
    let msgs = lower_errors(quote! {
        name: G;
        input event a;
        nodes {
            fast = Src::new() * 2;
            sink = Sink::new();
        }
        connections {
            a -> sink.ev;
            fast.ev -> sink.ev;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("event fan-in merges only same-rate scalar event sources")
            && m.contains("a cross-rate event edge")),
        "got {msgs:?}"
    );
}

#[test]
fn compound_source_in_stream_fan_in_is_a_lowering_diagnostic() {
    // Previously codegen emitted `compile_error!` tokens for this; it must
    // now be an ordinary accumulated diagnostic from lowering.
    let msgs = lower_errors(quote! {
        name: G;
        input value gain = 0.5;
        output stream out;
        nodes {
            osc = Src::new();
            osc2 = Src::new();
            filter = Gain::new(1.0);
        }
        connections {
            osc.output * gain -> filter.input;
            osc2.output -> filter.input;
            filter.output -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("saw a compound (non-endpoint) source into `filter.input`")),
        "got {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Broadcast + indexed overlap
// ---------------------------------------------------------------------------

#[test]
fn broadcast_and_indexed_stream_drivers_accumulate() {
    let ir = lower_ok(quote! {
        name: G;
        input stream a;
        input stream b;
        output stream out;
        node voices = [Gain::new(1.0); 4];
        connections {
            b -> voices[0].input;
            a -> voices.input;
            voices[0].output -> out;
        }
    });
    let all = group_for(&ir, "voices", "input", None);
    let idx = group_for(&ir, "voices", "input", Some(0));
    assert_eq!(all.policy, FanInPolicy::Single);
    assert!(idx.onto_broadcast, "indexed group must join the broadcast");
    assert_eq!(idx.policy, FanInPolicy::Sum);
    assert!(idx.accumulates());
    // Emission order puts the broadcast first even though the indexed
    // connection was declared first.
    let voices = node_id(&ir, "voices");
    let order: Vec<Element> = ir
        .drivers
        .groups_for_node(voices)
        .map(|g| g.dest.element)
        .collect();
    assert_eq!(order, vec![Element::All { n: 4 }, Element::Index(0)]);
    // The canonical group list is still in edge order.
    assert!(matches!(ir.drivers.groups[0].dest.element, Element::Index(0)));
}

#[test]
fn broadcast_and_indexed_value_drivers_are_rejected() {
    let msgs = lower_errors(quote! {
        name: G;
        input value all;
        input value one;
        output stream out;
        node voices = [Gain::new(1.0); 4];
        connections {
            all -> voices.gain;
            one -> voices[0].gain;
            voices[0].output -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("driven both directly and by a broadcast connection")),
        "got {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Address resolution
// ---------------------------------------------------------------------------

#[test]
fn indexed_destination_resolves_to_one_element() {
    let ir = lower_ok(quote! {
        name: G;
        input value gain = 2.0;
        output stream out;
        node voices = [Gain::new(1.0); 4];
        connections {
            gain * 0.5 -> voices[2].input;
            voices[2].output -> out;
        }
    });
    let g = group_for(&ir, "voices", "input", Some(2));
    assert_eq!(g.dest.element, Element::Index(2));
    assert_eq!(g.dest.channel, None);
    assert_eq!(g.policy, FanInPolicy::Single);
}

#[test]
fn channel_index_on_scalar_source_is_a_channel_not_an_element() {
    let ir = lower_ok(quote! {
        name: G;
        input stream s: Frame<2>;
        output stream out;
        node g = Gain::new(1.0);
        connections {
            s[1] -> g.input;
            g.output -> out;
        }
    });
    let eid = ir.edge_order[0];
    let src = match &ir.edges[eid].source.kind {
        ir::IrExprKind::Endpoint(ep) => ir::Address::resolve(ep, &ir),
        _ => panic!("simple source"),
    };
    assert_eq!(src.element, Element::Scalar);
    assert_eq!(src.channel, Some(1));
}

// ---------------------------------------------------------------------------
// Expression clock
// ---------------------------------------------------------------------------

fn mixed_rate_graph(first_fast: bool) -> proc_macro2::TokenStream {
    if first_fast {
        quote! {
            name: G;
            output stream out;
            nodes {
                slow = Src::new();
                fast = Src::new() * 4;
                sink = Gain::new(1.0);
            }
            connections {
                fast.output + slow.output -> sink.input;
                sink.output -> out;
            }
        }
    } else {
        quote! {
            name: G;
            output stream out;
            nodes {
                slow = Src::new();
                fast = Src::new() * 4;
                sink = Gain::new(1.0);
            }
            connections {
                slow.output + fast.output -> sink.input;
                sink.output -> out;
            }
        }
    }
}

#[test]
fn mixed_rate_expression_is_rejected_in_both_operand_orders() {
    let a = lower_errors(mixed_rate_graph(false));
    let b = lower_errors(mixed_rate_graph(true));
    let pick = |msgs: &[String]| {
        msgs.iter()
            .find(|m| m.contains("expression mixes nodes at different rates"))
            .cloned()
            .unwrap_or_else(|| panic!("no mixed-rate diagnostic in {msgs:?}"))
    };
    let ma = pick(&a);
    let mb = pick(&b);
    assert!(ma.contains("`slow` at the base rate") && ma.contains("`fast` at `* 4`"), "{ma}");
    assert!(mb.contains("`slow` at the base rate") && mb.contains("`fast` at `* 4`"), "{mb}");
}

#[test]
fn value_input_times_fast_node_downsamples_regardless_of_operand_order() {
    // `gain * fast.output` used to anchor on the value input (base rate) and
    // get no kernel at all, reading the fast node undownsampled; the
    // reversed spelling got `Down { 4 }`. Both must resolve to the fast
    // clock now.
    for tokens in [
        quote! {
            name: G;
            input value gain = 1.0;
            output stream out;
            nodes {
                fast = Src::new() * 4;
                sink = Gain::new(1.0);
            }
            connections {
                gain * fast.output -> sink.input;
                sink.output -> out;
            }
        },
        quote! {
            name: G;
            input value gain = 1.0;
            output stream out;
            nodes {
                fast = Src::new() * 4;
                sink = Gain::new(1.0);
            }
            connections {
                fast.output * gain -> sink.input;
                sink.output -> out;
            }
        },
    ] {
        let ir = lower_ok(tokens);
        let eid = ir.edge_order[0];
        let edge = &ir.edges[eid];
        assert_eq!(edge.source_rate, NodeRate::Up(4));
        assert!(
            matches!(edge.kernel, EdgeKernel::Down { factor: 4, .. }),
            "expected a Down{{4}} kernel; got {:?}",
            edge.kernel
        );
    }
}

#[test]
fn neutral_operands_resolve_to_the_base_rate() {
    let ir = lower_ok(quote! {
        name: G;
        input value gain = 2.0;
        output stream out;
        node g = Gain::new(1.0);
        connections {
            gain * 0.5 -> g.input;
            g.output -> out;
        }
    });
    let edge = &ir.edges[ir.edge_order[0]];
    assert_eq!(edge.source_rate, NodeRate::Same);
    assert_eq!(edge.kernel, EdgeKernel::None);
}

#[test]
fn same_rate_operands_agree() {
    let ir = lower_ok(quote! {
        name: G;
        output stream out;
        nodes {
            a = Src::new() * 2;
            b = Src::new() * 2;
            sink = Gain::new(1.0);
        }
        connections {
            a.output + b.output -> sink.input;
            sink.output -> out;
        }
    });
    let edge = &ir.edges[ir.edge_order[0]];
    assert_eq!(edge.source_rate, NodeRate::Up(2));
    assert!(matches!(edge.kernel, EdgeKernel::Down { factor: 2, .. }));
}

#[test]
fn unindexed_array_inside_expression_is_rejected() {
    let msgs = lower_errors(quote! {
        name: G;
        output stream out;
        node voices = [Gain::new(1.0); 4];
        connections {
            voices.output * 0.5 -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("`voices` is a node array") && m.contains("voices[0].endpoint")),
        "got {msgs:?}"
    );
}

#[test]
fn hoisted_value_endpoint_with_node_driver_is_rejected_at_macro_time() {
    // The explicit hoist anchors `osc.frequency` as a known value endpoint,
    // so the node-to-node driver is rejected here, not by rustc.
    let msgs = lower_errors(quote! {
        name: G;
        output stream out;
        nodes {
            lfo = Src::new();
            osc = Osc::new();
        }
        input osc.frequency freq;
        connections {
            lfo.output -> osc.frequency;
            osc.output -> out;
        }
    });
    // Hoist expansion catches the overlap first with its own wording; the
    // point is that it is a macro-time diagnostic, not a rustc error.
    assert!(
        msgs.iter().any(|m| m.contains("value endpoint `osc.frequency` has 2 sources")
            || (m.contains("re-exports `osc.frequency`") && m.contains("two drivers"))),
        "got {msgs:?}"
    );
}
