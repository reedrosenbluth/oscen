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
}
