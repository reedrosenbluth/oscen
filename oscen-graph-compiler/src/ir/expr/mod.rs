//! Typed expression IR for `IrEdge` source/destination.
//!
//! Replaces the raw `ConnectionExpr` AST that `IrEdge` carried in Phase 3.
//! Endpoint references are resolved (`NodeId` + endpoint name + optional
//! array index). Leaves that aren't endpoint references (literals, method
//! call args) keep `syn::Expr` because codegen has to emit valid Rust at
//! those points.

use crate::ast::BinaryOp;
use crate::ir::graph::NodeId;
use proc_macro2::Span;
use quote::ToTokens;
use syn::Ident;

pub mod visit;

/// Resolved endpoint reference. The optional `index` is `Some(k)` for array
/// indexed references like `voices[k].field`, `None` for scalar nodes.
///
/// `bare` is true when this endpoint was lowered from a `ConnectionExpr::Ident`
/// (a graph input/output referenced without a field selector). Emission uses
/// `self.<node>` for these instead of `self.<node>.<endpoint>`.
#[derive(Clone, Debug)]
pub struct IrEndpoint {
    pub node: NodeId,
    pub endpoint: Ident,
    pub index: Option<usize>,
    pub span: Span,
    pub bare: bool,
}

/// Span-bearing wrapper. Span lives uniformly here so the kind enum stays
/// clean and every visitor / pass gets a consistent diagnostic anchor.
#[derive(Clone)]
pub struct IrExpr {
    pub kind: IrExprKind,
    pub span: Span,
}

impl std::fmt::Debug for IrExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrExpr")
            .field("kind", &self.kind)
            .field("span", &self.span)
            .finish()
    }
}

#[derive(Clone)]
pub enum IrExprKind {
    /// `cutoff`, `osc.output`, `voices[0].output` — all collapse to this.
    Endpoint(IrEndpoint),

    /// `a * b`, `a + b`, etc.
    Binary {
        left: Box<IrExpr>,
        op: BinaryOp,
        right: Box<IrExpr>,
    },

    /// `x.tanh()`, `(a*b).clamp(0.0, 1.0)`. Args stay as raw `syn::Expr`
    /// because they're opaque Rust the macro passes through verbatim.
    MethodCall {
        receiver: Box<IrExpr>,
        method: Ident,
        args: Vec<syn::Expr>,
    },

    /// `tanh(x)`, `clamp(x, 0.0, 1.0)`. Args are `IrExpr` because they can
    /// reference endpoints (`tanh(osc.output)`).
    Call { function: Ident, args: Vec<IrExpr> },

    /// `0.5`, `2.0 * PI` — opaque Rust that references no endpoint.
    Literal(syn::Expr),
}

impl std::fmt::Debug for IrExprKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrExprKind::Endpoint(ep) => f.debug_tuple("Endpoint").field(ep).finish(),
            IrExprKind::Binary { left, op, right } => f
                .debug_struct("Binary")
                .field("left", left)
                .field("op", op)
                .field("right", right)
                .finish(),
            IrExprKind::MethodCall {
                receiver,
                method,
                args,
            } => f
                .debug_struct("MethodCall")
                .field("receiver", receiver)
                .field("method", method)
                .field("args", &format!("<{} syn::Expr args>", args.len()))
                .finish(),
            IrExprKind::Call { function, args } => f
                .debug_struct("Call")
                .field("function", function)
                .field("args", args)
                .finish(),
            IrExprKind::Literal(expr) => f
                .debug_tuple("Literal")
                .field(&expr.to_token_stream().to_string())
                .finish(),
        }
    }
}

/// Walk an `IrExpr` to find the leftmost endpoint's `NodeId`. Descends through
/// `Binary` (left) and `MethodCall` (receiver). Returns `None` for `Call` and
/// `Literal` (no leftmost node reference).
pub(crate) fn primary_node(expr: &IrExpr) -> Option<NodeId> {
    match &expr.kind {
        IrExprKind::Endpoint(ep) => Some(ep.node),
        IrExprKind::Binary { left, .. } => primary_node(left),
        IrExprKind::MethodCall { receiver, .. } => primary_node(receiver),
        IrExprKind::Call { .. } | IrExprKind::Literal(_) => None,
    }
}
