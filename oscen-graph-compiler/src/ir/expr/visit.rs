//! Visitor pattern for `IrExpr`.
//!
//! Provides a single source of recursion for passes that need to walk an
//! expression tree. Both `Visitor` (`&IrExpr`) and `MutVisitor` (`&mut IrExpr`)
//! are provided. Default methods recurse via `walk_expr` / `walk_expr_mut`.

use crate::ir::expr::{IrEndpoint, IrExpr, IrExprKind};

pub trait Visitor {
    fn visit_expr(&mut self, expr: &IrExpr) {
        walk_expr(self, expr);
    }

    fn visit_endpoint(&mut self, _ep: &IrEndpoint) {}
}

pub fn walk_expr<V: Visitor + ?Sized>(v: &mut V, expr: &IrExpr) {
    match &expr.kind {
        IrExprKind::Endpoint(ep) => v.visit_endpoint(ep),
        IrExprKind::Binary { left, right, .. } => {
            v.visit_expr(left);
            v.visit_expr(right);
        }
        IrExprKind::MethodCall { receiver, .. } => {
            v.visit_expr(receiver);
        }
        IrExprKind::Call { args, .. } => {
            for a in args {
                v.visit_expr(a);
            }
        }
        IrExprKind::Literal(_) => {}
    }
}

pub trait MutVisitor {
    fn visit_expr_mut(&mut self, expr: &mut IrExpr) {
        walk_expr_mut(self, expr);
    }

    fn visit_endpoint_mut(&mut self, _ep: &mut IrEndpoint) {}
}

pub fn walk_expr_mut<V: MutVisitor + ?Sized>(v: &mut V, expr: &mut IrExpr) {
    match &mut expr.kind {
        IrExprKind::Endpoint(ep) => v.visit_endpoint_mut(ep),
        IrExprKind::Binary { left, right, .. } => {
            v.visit_expr_mut(left);
            v.visit_expr_mut(right);
        }
        IrExprKind::MethodCall { receiver, .. } => {
            v.visit_expr_mut(receiver);
        }
        IrExprKind::Call { args, .. } => {
            for a in args {
                v.visit_expr_mut(a);
            }
        }
        IrExprKind::Literal(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinaryOp;
    use crate::ir::expr::{IrEndpoint, IrExpr, IrExprKind};
    use crate::ir::graph::NodeId;
    use proc_macro2::Span;
    use quote::format_ident;
    use slotmap::KeyData;

    fn dummy_node_id() -> NodeId {
        NodeId::from(KeyData::from_ffi(1))
    }

    fn endpoint(name: &str) -> IrExpr {
        IrExpr {
            kind: IrExprKind::Endpoint(IrEndpoint {
                node: dummy_node_id(),
                endpoint: format_ident!("{}", name),
                index: None,
                span: Span::call_site(),
                bare: false,
            }),
            span: Span::call_site(),
        }
    }

    struct Counter(usize);
    impl Visitor for Counter {
        fn visit_endpoint(&mut self, _ep: &IrEndpoint) {
            self.0 += 1;
        }
    }

    #[test]
    fn walk_expr_visits_all_endpoints_in_binary() {
        let expr = IrExpr {
            kind: IrExprKind::Binary {
                left: Box::new(endpoint("a")),
                op: BinaryOp::Mul,
                right: Box::new(endpoint("b")),
            },
            span: Span::call_site(),
        };
        let mut counter = Counter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 2);
    }

    #[test]
    fn walk_expr_visits_nested_call_args() {
        let expr = IrExpr {
            kind: IrExprKind::Call {
                function: syn::parse_quote!(clamp),
                args: vec![endpoint("x"), endpoint("y"), endpoint("z")],
            },
            span: Span::call_site(),
        };
        let mut counter = Counter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 3);
    }

    #[test]
    fn walk_expr_does_not_visit_literal() {
        let expr = IrExpr {
            kind: IrExprKind::Literal(syn::parse_quote!(0.5)),
            span: Span::call_site(),
        };
        let mut counter = Counter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 0);
    }

    #[test]
    fn walk_expr_visits_method_call_receiver_but_not_args() {
        let expr = IrExpr {
            kind: IrExprKind::MethodCall {
                receiver: Box::new(endpoint("x")),
                method: format_ident!("tanh"),
                args: vec![], // MethodCall args are opaque syn::Expr; visitor doesn't recurse into them
            },
            span: Span::call_site(),
        };
        let mut counter = Counter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 1, "receiver should be visited");
    }
}
