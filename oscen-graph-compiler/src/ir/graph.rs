//! IR data structures.
//!
//! `IrGraph` is the unified post-lowering representation. Every per-node
//! and per-edge fact (kind, rate, kernel, fanout, span) lives on the
//! record itself — no parallel side-tables. Mutation goes through
//! `remove_node` / `remove_edge`, which maintain adjacency, topological
//! order, and reference-validity invariants.

use crate::ast::{ConnectionPolicy, EndpointKind, NodeRate};
use proc_macro2::Span;
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;
use syn::{Expr, Ident, Path};

// ---------------------------------------------------------------------------
// Edge-type enums (moved here from legacy rate_analysis / fanout modules)
// ---------------------------------------------------------------------------

/// Per-edge frame_offset rescaling for event-typed cross-rate edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRescale {
    /// Same-rate edge: no rescaling applied.
    None,
    /// Outer -> inner: multiply offsets by N.
    Multiply(u32),
    /// Inner -> outer: divide offsets by N.
    Divide(u32),
}

/// Resampling kernel selection for a single cross-rate edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKernel {
    /// No conversion needed (same rate, or both directions are no-op).
    None,
    /// Upsample: source slower, dest faster.
    Up { factor: u32, kind: ConnectionPolicy },
    /// Downsample: source faster, dest slower.
    Down { factor: u32, kind: ConnectionPolicy },
    /// Event-typed edge. Same-rate (`rescale = None`) is functionally
    /// equivalent to `EdgeKernel::None` and emits a plain copy via the
    /// existing event try_push path; cross-rate variants emit the same
    /// try_push loop but transform `EventInstance::frame_offset` per
    /// `rescale` so events fire on the correct inner/outer tick.
    Event { rescale: EventRescale },
}

/// Per-edge fan-out shape: how many source values feed how many dest slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutShape {
    /// Both sides scalar (or expression with no array root).
    Scalar,
    /// Both sides arrays of equal size N: parallel — one resampler per element.
    Parallel { n: usize },
    /// Scalar src → array dest of size N: broadcast — shared resampler, N dest writes.
    Broadcast { n: usize },
    /// Array src of size N → scalar dest: fan-in — sum sources first, then shared resampler.
    FanIn { n: usize },
}

/// Classify a connection edge given the array sizes of its source and dest
/// nodes (`None` for scalar nodes or graph endpoints).
///
/// For mismatched-but-nonzero array sizes, parity with the same-rate path's
/// existing behavior: silently truncate to `min(N, M)` (`Parallel { n: min }`).
/// Promoting this to a hard error is reserved for a future task.
pub fn classify_fanout(
    src_array_size: Option<usize>,
    dst_array_size: Option<usize>,
) -> FanoutShape {
    use FanoutShape::*;
    match (src_array_size, dst_array_size) {
        (None, None) => Scalar,
        (None, Some(n)) => Broadcast { n },
        (Some(n), None) => FanIn { n },
        (Some(n), Some(m)) if n == m => Parallel { n },
        (Some(n), Some(m)) => Parallel { n: n.min(m) },
    }
}

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
    /// Edges in canonical source order (populated by `lower::build_edges`).
    /// This is the deterministic iteration order codegen uses to index
    /// per-edge resampler fields and buffers. Removing an edge via
    /// `remove_edge` keeps the surviving edges' relative order.
    pub edge_order: Vec<EdgeId>,
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

#[allow(clippy::large_enum_variant)]
pub enum IrNodeKind {
    Input {
        spec: Option<crate::ast::ParamSpec>,
        default: Option<Expr>,
    },
    Output,
    Processor {
        ty: Option<Path>,
        /// Raw constructor `syn::Expr`. Preserves Path-vs-Call distinction
        /// so codegen can emit `Type::new()` for bare paths and pass
        /// through call expressions unchanged.
        ctor_expr: Expr,
    },
    NodeArray {
        ty: Option<Path>,
        /// Raw constructor `syn::Expr` (same rationale as `Processor`).
        ctor_expr: Expr,
        len: usize,
    },
}

pub struct EndpointInfo {
    pub kind: EndpointKind,
}

pub struct IrEdge {
    pub id: EdgeId,
    /// Resolved source expression. For simple `node.field` connections this
    /// is `IrExprKind::Endpoint`; for compound expressions (binary ops,
    /// method calls) it is the structured `IrExpr` tree. The primary
    /// referenced node is the leftmost `Endpoint` leaf; additional
    /// referenced nodes are cached in `extra_source_nodes`.
    pub source: crate::ir::expr::IrExpr,
    /// Resolved destination endpoint (addressable: `out`, `node.field`,
    /// or `voices[k].field`).
    pub dest: crate::ir::expr::IrEndpoint,
    pub policy: ConnectionPolicy,
    pub kernel: EdgeKernel,
    pub fanout: FanoutShape,
    pub span: Span,
    /// Secondary referenced source nodes (for compound expressions like
    /// `a.x * b.y -> out`, this contains every additional `NodeId`
    /// referenced by the source expression beyond the primary). Empty for
    /// simple `node.endpoint` sources. Used by dead-node analysis so
    /// nodes whose values feed an expression are kept alive.
    pub extra_source_nodes: Vec<NodeId>,
    /// True for the outgoing leg of an inline-delay edge (`-> [N] ->` or
    /// `-> [name] ->`). Feedback edges are skipped during topological
    /// ordering and trigger emission of an `AllowsFeedback` static-bound
    /// check on the source's primary node type.
    pub is_feedback: bool,
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
            edge_order: Vec::new(),
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
        // Clean up the primary source node's outgoing list.
        if let Some(primary) = crate::ir::expr::primary_node(&edge.source) {
            if let Some(src_node) = self.nodes.get_mut(primary) {
                src_node.outgoing.retain(|&e| e != id);
            }
        }
        // Clean up extra source nodes' outgoing lists.
        for extra in &edge.extra_source_nodes {
            if let Some(extra_node) = self.nodes.get_mut(*extra) {
                extra_node.outgoing.retain(|&e| e != id);
            }
        }
        if let Some(dst_node) = self.nodes.get_mut(edge.dest.node) {
            dst_node.incoming.retain(|&e| e != id);
        }
        self.edge_order.retain(|&e| e != id);
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
                ty: Some(parse_quote!(Dummy)),
                ctor_expr: parse_quote!(Dummy {}),
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
            source: crate::ir::expr::IrExpr {
                kind: crate::ir::expr::IrExprKind::Endpoint(crate::ir::expr::IrEndpoint {
                    node: source,
                    endpoint: format_ident!("out"),
                    index: None,
                    span: Span::call_site(),
                    bare: false,
                }),
                span: Span::call_site(),
            },
            dest: crate::ir::expr::IrEndpoint {
                node: dest,
                endpoint: format_ident!("in"),
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
        graph.nodes[source].outgoing.push(id);
        graph.nodes[dest].incoming.push(id);
        graph.edge_order.push(id);
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
        assert!(
            !g.processors.contains(&b),
            "b should be removed from processors[]"
        );
        assert!(
            g.edges.values().count() == 0,
            "both incident edges should be gone"
        );
        assert!(
            g.nodes[a].outgoing.is_empty(),
            "a's outgoing should be cleaned"
        );
        assert!(
            g.nodes[c].incoming.is_empty(),
            "c's incoming should be cleaned"
        );
    }
}
