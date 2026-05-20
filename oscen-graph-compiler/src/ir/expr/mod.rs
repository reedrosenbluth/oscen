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
use syn::Ident;

pub mod visit;

/// Resolved endpoint reference. The optional `index` is `Some(k)` for array
/// indexed references like `voices[k].field`, `None` for scalar nodes.
#[derive(Clone, Debug)]
pub struct IrEndpoint {
    pub node: NodeId,
    pub endpoint: Ident,
    pub index: Option<usize>,
    pub span: Span,
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
    Call {
        function: Ident,
        args: Vec<IrExpr>,
    },

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
            IrExprKind::Literal(_) => f.debug_tuple("Literal").field(&"<syn::Expr>").finish(),
        }
    }
}
