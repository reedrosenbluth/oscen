//! Dead-node removal pass.
//!
//! Removes processor / node-array nodes whose outputs don't transitively
//! reach any graph output. Reverse BFS from outputs, O(V + E). Cmajor
//! parity for `RemoveUnusedNodes`.

use crate::ir::expr::primary_node;
use crate::ir::graph::{IrGraph, IrNodeKind, NodeId};
use std::collections::{HashSet, VecDeque};

pub fn run(ir: &mut IrGraph) {
    // Conservative guard: if a graph has no declared outputs, leave every
    // node intact. Sink-less graphs are typically test fixtures or graphs
    // whose effects (e.g., feeding into named node fields read by external
    // code) aren't visible via the IR's output set. Pruning them would
    // remove fields the user reaches via `graph.<node>.<field>` directly.
    if ir.outputs.is_empty() {
        return;
    }

    // Mark live: start from each Output node; walk backward via incoming
    // edges; mark every node visited as live. Compound-source edges carry
    // additional referenced nodes in `extra_source_nodes` — walk those too
    // so e.g. `a.x * b.y -> out` keeps both `a` and `b` alive.
    let mut live: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<NodeId> = ir.outputs.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        if !live.insert(id) {
            continue;
        }
        // Collect first to avoid borrow conflict during graph mutation later.
        let incoming = ir.nodes[id].incoming.to_vec();
        for eid in incoming {
            let edge = &ir.edges[eid];
            // Push the primary source node (leftmost endpoint in the IrExpr).
            if let Some(primary) = primary_node(&edge.source) {
                queue.push_back(primary);
            }
            // Push any extra source nodes (for compound exprs like a.x * b.y).
            for &extra in &edge.extra_source_nodes {
                queue.push_back(extra);
            }
        }
    }

    // Identify dead processor / node-array nodes (Input/Output are never candidates).
    let dead: Vec<NodeId> = ir
        .nodes
        .iter()
        .filter(|(id, node)| {
            matches!(
                node.kind,
                IrNodeKind::Processor { .. } | IrNodeKind::NodeArray { .. }
            ) && !live.contains(id)
        })
        .map(|(id, _)| id)
        .collect();
    for id in dead {
        ir.remove_node(id);
    }

    #[cfg(debug_assertions)]
    crate::ir::validate::validate(ir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{ConnectionPolicy, EndpointKind, NodeRate};
    use crate::ir::graph::{
        EdgeId, EdgeKernel, EndpointInfo, FanoutShape, IrEdge, IrNode, IrNodeKind,
    };
    use proc_macro2::Span;
    use quote::format_ident;
    use std::collections::HashMap;
    use syn::parse_quote;

    fn add_input(g: &mut IrGraph, name: &str, endpoint: &str) -> NodeId {
        let id = g.nodes.insert_with_key(|id| IrNode {
            id,
            kind: IrNodeKind::Input {
                spec: None,
                default: None,
            },
            name: format_ident!("{}", name),
            rate: NodeRate::Same,
            latency_samples: 0,
            span: Span::call_site(),
            endpoints: {
                let mut m = HashMap::new();
                m.insert(
                    format_ident!("{}", endpoint),
                    EndpointInfo {
                        kind: EndpointKind::Stream,
                    },
                );
                m
            },
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        g.inputs.push(id);
        id
    }

    fn add_output(g: &mut IrGraph, name: &str) -> NodeId {
        let id = g.nodes.insert_with_key(|id| IrNode {
            id,
            kind: IrNodeKind::Output,
            name: format_ident!("{}", name),
            rate: NodeRate::Same,
            latency_samples: 0,
            span: Span::call_site(),
            endpoints: {
                let mut m = HashMap::new();
                m.insert(
                    format_ident!("{}", name),
                    EndpointInfo {
                        kind: EndpointKind::Stream,
                    },
                );
                m
            },
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        g.outputs.push(id);
        id
    }

    fn add_processor(g: &mut IrGraph, name: &str) -> NodeId {
        let id = g.nodes.insert_with_key(|id| IrNode {
            id,
            kind: IrNodeKind::Processor {
                ty: Some(parse_quote!(Dummy)),
                ctor_expr: parse_quote!(Dummy {}),
            },
            name: format_ident!("{}", name),
            rate: NodeRate::Same,
            latency_samples: 0,
            span: Span::call_site(),
            endpoints: HashMap::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        g.processors.push(id);
        id
    }

    fn add_edge(g: &mut IrGraph, src: NodeId, src_ep: &str, dst: NodeId, dst_ep: &str) -> EdgeId {
        let src_ep_ident = format_ident!("{}", src_ep);
        let dst_ep_ident = format_ident!("{}", dst_ep);
        let id = g.edges.insert_with_key(|id| IrEdge {
            id,
            source: crate::ir::expr::IrExpr {
                kind: crate::ir::expr::IrExprKind::Endpoint(crate::ir::expr::IrEndpoint {
                    node: src,
                    endpoint: src_ep_ident,
                    index: None,
                    span: Span::call_site(),
                    bare: false,
                }),
                span: Span::call_site(),
            },
            dest: crate::ir::expr::IrEndpoint {
                node: dst,
                endpoint: dst_ep_ident,
                index: None,
                span: Span::call_site(),
                bare: false,
            },
            policy: ConnectionPolicy::Default,
            kernel: EdgeKernel::None,
            fanout: FanoutShape::Scalar,
            span: Span::call_site(),
            extra_source_nodes: Vec::new(),
            is_feedback: false,
        });
        g.nodes[src].outgoing.push(id);
        g.nodes[dst].incoming.push(id);
        g.edge_order.push(id);
        id
    }

    #[test]
    fn empty_graph_is_noop() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        run(&mut g);
        assert_eq!(g.nodes.len(), 0);
    }

    #[test]
    fn all_live_graph_is_noop() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let i = add_input(&mut g, "s", "s");
        let p = add_processor(&mut g, "p");
        let o = add_output(&mut g, "out");
        add_edge(&mut g, i, "s", p, "in");
        add_edge(&mut g, p, "out", o, "out");
        let before = g.nodes.len();
        run(&mut g);
        assert_eq!(g.nodes.len(), before);
    }

    #[test]
    fn single_dead_leaf_removed() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let i = add_input(&mut g, "s", "s");
        let p_live = add_processor(&mut g, "p_live");
        let p_dead = add_processor(&mut g, "p_dead");
        let o = add_output(&mut g, "out");
        add_edge(&mut g, i, "s", p_live, "in");
        add_edge(&mut g, i, "s", p_dead, "in");
        add_edge(&mut g, p_live, "out", o, "out");

        run(&mut g);
        assert!(g.nodes.get(p_dead).is_none(), "p_dead should be removed");
        assert!(g.nodes.get(p_live).is_some(), "p_live should remain");
    }

    #[test]
    fn dead_chain_all_removed() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let _o = add_output(&mut g, "out");
        let a = add_processor(&mut g, "a");
        let b = add_processor(&mut g, "b");
        let c = add_processor(&mut g, "c");
        add_edge(&mut g, a, "out", b, "in");
        add_edge(&mut g, b, "out", c, "in");

        run(&mut g);
        assert!(g.nodes.get(a).is_none());
        assert!(g.nodes.get(b).is_none());
        assert!(g.nodes.get(c).is_none());
    }

    #[test]
    fn dead_cycle_removed() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let _o = add_output(&mut g, "out");
        let a = add_processor(&mut g, "a");
        let b = add_processor(&mut g, "b");
        add_edge(&mut g, a, "out", b, "in");
        add_edge(&mut g, b, "out", a, "in");

        run(&mut g);
        assert!(g.nodes.get(a).is_none());
        assert!(g.nodes.get(b).is_none());
    }

    #[test]
    fn mixed_live_trunk_and_dead_branch() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let i = add_input(&mut g, "s", "s");
        let trunk = add_processor(&mut g, "trunk");
        let branch = add_processor(&mut g, "branch");
        let o = add_output(&mut g, "out");
        add_edge(&mut g, i, "s", trunk, "in");
        add_edge(&mut g, trunk, "out", branch, "in");
        add_edge(&mut g, trunk, "out", o, "out");

        run(&mut g);
        assert!(g.nodes.get(trunk).is_some());
        assert!(g.nodes.get(branch).is_none());
    }

    #[test]
    fn node_feeding_live_endpoint_is_kept_alive() {
        // Any node with an outgoing edge into a live destination is itself
        // live — the reverse-BFS walks every incoming edge of a live node,
        // so this source gets marked live in the first hop.
        let mut g = IrGraph::new(format_ident!("G"), false);
        let i = add_input(&mut g, "s", "s");
        let live = add_processor(&mut g, "live");
        let src = add_processor(&mut g, "src");
        let o = add_output(&mut g, "out");
        add_edge(&mut g, i, "s", live, "in");
        add_edge(&mut g, src, "out", live, "feedback_in");
        add_edge(&mut g, live, "out", o, "out");

        run(&mut g);
        assert!(
            g.nodes.get(src).is_some(),
            "src feeds a live endpoint and must be kept"
        );
        assert!(g.nodes.get(live).is_some());
    }

    #[test]
    fn dead_node_array_removed() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let _o = add_output(&mut g, "out");
        let id = g.nodes.insert_with_key(|id| IrNode {
            id,
            kind: IrNodeKind::NodeArray {
                ty: Some(parse_quote!(Voice)),
                ctor_expr: parse_quote!(Voice {}),
                len: 8,
            },
            name: format_ident!("voices"),
            rate: NodeRate::Same,
            latency_samples: 0,
            span: Span::call_site(),
            endpoints: HashMap::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        g.processors.push(id);

        run(&mut g);
        assert!(
            g.nodes.get(id).is_none(),
            "dead NodeArray should be removed"
        );
    }
}
