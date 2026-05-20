//! Debug-only IR invariant checks.
//!
//! `validate(&IrGraph)` panics if the IR is internally inconsistent.
//! Called inside `lower()` and after every pass under
//! `cfg(debug_assertions)`. Zero release-build cost.

use crate::ir::graph::{EdgeId, IrGraph, NodeId};
use std::collections::HashSet;

/// Extract the primary (leftmost) `NodeId` from an `IrExpr`.
fn primary_source_node_of_expr(expr: &crate::ir::expr::IrExpr) -> Option<NodeId> {
    use crate::ir::expr::IrExprKind;
    match &expr.kind {
        IrExprKind::Endpoint(ep) => Some(ep.node),
        IrExprKind::Binary { left, .. } => primary_source_node_of_expr(left),
        IrExprKind::MethodCall { receiver, .. } => primary_source_node_of_expr(receiver),
        IrExprKind::Call { .. } | IrExprKind::Literal(_) => None,
    }
}

/// Collect all `NodeId`s referenced by an `IrExpr` source expression.
fn collect_source_node_ids_for_validate(expr: &crate::ir::expr::IrExpr) -> Vec<NodeId> {
    use crate::ir::expr::IrExprKind;
    let mut ids = Vec::new();
    fn walk(expr: &crate::ir::expr::IrExpr, ids: &mut Vec<NodeId>) {
        match &expr.kind {
            IrExprKind::Endpoint(ep) => ids.push(ep.node),
            IrExprKind::Binary { left, right, .. } => {
                walk(left, ids);
                walk(right, ids);
            }
            IrExprKind::MethodCall { receiver, .. } => walk(receiver, ids),
            IrExprKind::Call { function: _, args } => {
                for arg in args {
                    walk(arg, ids);
                }
            }
            IrExprKind::Literal(_) => {}
        }
    }
    walk(expr, &mut ids);
    ids
}

pub fn validate(ir: &IrGraph) {
    let edge_set: HashSet<EdgeId> = ir.edges.keys().collect();

    for (nid, node) in &ir.nodes {
        // Adjacency entries point at live edges.
        for &eid in &node.incoming {
            assert!(
                edge_set.contains(&eid),
                "node {nid:?}.incoming contains stale edge {eid:?}"
            );
            assert!(
                ir.edges[eid].dest.node == nid,
                "node {nid:?}.incoming contains edge {eid:?} whose dest is not this node"
            );
        }
        for &eid in &node.outgoing {
            assert!(
                edge_set.contains(&eid),
                "node {nid:?}.outgoing contains stale edge {eid:?}"
            );
            let edge = &ir.edges[eid];
            let is_primary = primary_source_node_of_expr(&edge.source) == Some(nid);
            let is_extra = edge.extra_source_nodes.contains(&nid);
            assert!(
                is_primary || is_extra,
                "node {nid:?}.outgoing contains edge {eid:?} whose source is not this node \
                 (and is not in extra_source_nodes either)"
            );
        }
    }

    // Edges reference live nodes.
    let node_set: HashSet<NodeId> = ir.nodes.keys().collect();
    for (eid, edge) in &ir.edges {
        // Check that every NodeId referenced by the source IrExpr is live.
        let source_refs = collect_source_node_ids_for_validate(&edge.source);
        for src_nid in &source_refs {
            assert!(
                node_set.contains(src_nid),
                "edge {eid:?}.source references dead node {src_nid:?}"
            );
        }
        assert!(
            node_set.contains(&edge.dest.node),
            "edge {eid:?}.dest references dead node {:?}",
            edge.dest.node
        );
        for &extra in &edge.extra_source_nodes {
            assert!(
                node_set.contains(&extra),
                "edge {eid:?}.extra_source_nodes references dead node {extra:?}"
            );
        }
    }

    // processors / inputs / outputs vectors reference live nodes.
    for &nid in &ir.processors {
        assert!(
            node_set.contains(&nid),
            "processors[] contains dead node {nid:?}"
        );
    }
    for &nid in &ir.inputs {
        assert!(
            node_set.contains(&nid),
            "inputs[] contains dead node {nid:?}"
        );
    }
    for &nid in &ir.outputs {
        assert!(
            node_set.contains(&nid),
            "outputs[] contains dead node {nid:?}"
        );
    }

    // edge_order references live edges and contains every live edge.
    for &eid in &ir.edge_order {
        assert!(
            edge_set.contains(&eid),
            "edge_order contains dead edge {eid:?}"
        );
    }
    assert_eq!(
        ir.edge_order.len(),
        ir.edges.len(),
        "edge_order must contain every live edge exactly once"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::graph::IrGraph;
    use quote::format_ident;

    #[test]
    fn empty_graph_validates() {
        let g = IrGraph::new(format_ident!("Empty"), false);
        validate(&g);
    }

    #[test]
    #[should_panic(expected = "contains dead node")]
    fn dead_processor_id_in_vec_panics() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        // Synthesize a dead NodeId by inserting then removing.
        let id = g.nodes.insert_with_key(|id| crate::ir::graph::IrNode {
            id,
            kind: crate::ir::graph::IrNodeKind::Output,
            name: format_ident!("dummy"),
            rate: crate::ast::NodeRate::Same,
            latency_samples: 0,
            span: proc_macro2::Span::call_site(),
            endpoints: Default::default(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        g.processors.push(id);
        g.nodes.remove(id); // now processors[] is stale
        validate(&g);
    }
}
