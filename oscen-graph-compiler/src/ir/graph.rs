//! IR data structures.
//!
//! `IrGraph` is the unified post-lowering representation. Every per-node
//! and per-edge fact (kind, rate, kernel, fanout, span) lives on the
//! record itself — no parallel side-tables. Mutation goes through
//! `remove_node` / `remove_edge`, which maintain adjacency, topological
//! order, and reference-validity invariants.

use crate::ast::{ConnectionExpr, ConnectionPolicy, EndpointKind, NodeRate};
use crate::fanout::FanoutShape;
use crate::rate_analysis::EdgeKernel;
use proc_macro2::{Span, TokenStream};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;
use syn::{Ident, Path};

new_key_type! {
    pub struct NodeId;
    pub struct EdgeId;
}

pub struct IrGraph {
    pub name: Ident,
    pub nih_params: bool,
    pub nodes: SlotMap<NodeId, IrNode>,
    pub edges: SlotMap<EdgeId, IrEdge>,
    /// Graph-level `input` declarations, in source order.
    pub inputs: Vec<NodeId>,
    /// Graph-level `output` declarations, in source order.
    pub outputs: Vec<NodeId>,
    /// Internal processor / node-array instances, in topological order
    /// (populated by `lower::topo_sort`).
    pub processors: Vec<NodeId>,
}

pub struct IrNode {
    pub id: NodeId,
    pub kind: IrNodeKind,
    pub name: Ident,
    pub rate: NodeRate,
    pub latency_samples: u32,
    pub span: Span,
    pub endpoints: HashMap<Ident, EndpointInfo>,
    pub incoming: Vec<EdgeId>,
    pub outgoing: Vec<EdgeId>,
}

pub enum IrNodeKind {
    Input { spec: Option<crate::ast::ParamSpec> },
    Output,
    Processor { ty: Path, ctor: TokenStream },
    NodeArray { ty: Path, ctor: TokenStream, len: usize },
}

pub struct EndpointInfo {
    pub kind: EndpointKind,
}

pub struct IrEdge {
    pub id: EdgeId,
    pub source: EndpointRef,
    pub dest: EndpointRef,
    pub policy: ConnectionPolicy,
    pub kernel: EdgeKernel,
    pub fanout: FanoutShape,
    pub span: Span,
    /// Raw AST source expression. Phase 3 doesn't lift this to an
    /// IR-native `IrExpr`; codegen continues to consume `ConnectionExpr`
    /// as today. The seam is documented in the design spec.
    pub source_expr: ConnectionExpr,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EndpointRef {
    pub node: NodeId,
    pub endpoint: Ident,
}

impl IrGraph {
    pub fn new(name: Ident, nih_params: bool) -> Self {
        Self {
            name,
            nih_params,
            nodes: SlotMap::with_key(),
            edges: SlotMap::with_key(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            processors: Vec::new(),
        }
    }

    /// Remove an edge. Updates source and dest node adjacency lists.
    ///
    /// Panics in debug if `id` is unknown (release: silent no-op via `.remove`).
    pub fn remove_edge(&mut self, id: EdgeId) {
        let Some(edge) = self.edges.remove(id) else {
            debug_assert!(false, "remove_edge on unknown EdgeId");
            return;
        };
        if let Some(src_node) = self.nodes.get_mut(edge.source.node) {
            src_node.outgoing.retain(|&e| e != id);
        }
        if let Some(dst_node) = self.nodes.get_mut(edge.dest.node) {
            dst_node.incoming.retain(|&e| e != id);
        }
    }

    /// Remove a node and all incident edges. Also removes the node from
    /// `processors[]` if it was a processor / node-array entry.
    ///
    /// Panics in debug if `id` is unknown.
    pub fn remove_node(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id) else {
            debug_assert!(false, "remove_node on unknown NodeId");
            return;
        };
        // Collect incident edges before mutating (avoids borrow conflict).
        let incident: Vec<EdgeId> = node
            .incoming
            .iter()
            .chain(node.outgoing.iter())
            .copied()
            .collect();
        for e in incident {
            self.remove_edge(e);
        }
        self.nodes.remove(id);
        self.processors.retain(|&n| n != id);
        self.inputs.retain(|&n| n != id);
        self.outputs.retain(|&n| n != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::NodeRate;
    use proc_macro2::Span;
    use quote::format_ident;
    use syn::parse_quote;

    fn mk_processor_node(graph: &mut IrGraph, name: &str) -> NodeId {
        let id = graph.nodes.insert_with_key(|id| IrNode {
            id,
            kind: IrNodeKind::Processor {
                ty: parse_quote!(Dummy),
                ctor: quote::quote!(Dummy {}),
            },
            name: format_ident!("{}", name),
            rate: NodeRate::Same,
            latency_samples: 0,
            span: Span::call_site(),
            endpoints: Default::default(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
        });
        graph.processors.push(id);
        id
    }

    fn mk_edge(graph: &mut IrGraph, source: NodeId, dest: NodeId) -> EdgeId {
        let id = graph.edges.insert_with_key(|id| IrEdge {
            id,
            source: EndpointRef { node: source, endpoint: format_ident!("out") },
            dest: EndpointRef { node: dest, endpoint: format_ident!("in") },
            policy: ConnectionPolicy::Default,
            kernel: EdgeKernel::None,
            fanout: FanoutShape::Scalar,
            span: Span::call_site(),
            source_expr: ConnectionExpr::Ident(format_ident!("dummy")),
        });
        graph.nodes[source].outgoing.push(id);
        graph.nodes[dest].incoming.push(id);
        id
    }

    #[test]
    fn remove_edge_unlinks_from_both_endpoints() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let a = mk_processor_node(&mut g, "a");
        let b = mk_processor_node(&mut g, "b");
        let e = mk_edge(&mut g, a, b);

        g.remove_edge(e);

        assert!(g.edges.get(e).is_none());
        assert!(g.nodes[a].outgoing.is_empty());
        assert!(g.nodes[b].incoming.is_empty());
    }

    #[test]
    fn remove_node_removes_incident_edges_and_processors_entry() {
        let mut g = IrGraph::new(format_ident!("G"), false);
        let a = mk_processor_node(&mut g, "a");
        let b = mk_processor_node(&mut g, "b");
        let c = mk_processor_node(&mut g, "c");
        let _e_ab = mk_edge(&mut g, a, b);
        let _e_bc = mk_edge(&mut g, b, c);

        g.remove_node(b);

        assert!(g.nodes.get(b).is_none(), "node b should be gone");
        assert!(!g.processors.contains(&b), "b should be removed from processors[]");
        assert!(g.edges.values().count() == 0, "both incident edges should be gone");
        assert!(g.nodes[a].outgoing.is_empty(), "a's outgoing should be cleaned");
        assert!(g.nodes[c].incoming.is_empty(), "c's incoming should be cleaned");
    }
}
