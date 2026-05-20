//! Typed graph IR for the `graph!` DSL.
//!
//! Built by `lower::lower(graph_def)` from the AST after parsing. Codegen
//! consumes `&IrGraph` directly; intermediate optimization passes
//! mutate the graph through the disciplined API on `IrGraph` (see
//! `graph::IrGraph::remove_node` / `remove_edge`).

pub mod expr;
pub mod graph;
pub mod lower;
pub mod passes;
pub mod validate;

pub use expr::{IrEndpoint, IrExpr, IrExprKind};
pub use graph::{
    classify_fanout, EdgeId, EdgeKernel, EndpointInfo, EventRescale, FanoutShape,
    IrEdge, IrGraph, IrNode, IrNodeKind, NodeId,
};
