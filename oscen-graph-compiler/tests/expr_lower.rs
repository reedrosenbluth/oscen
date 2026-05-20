//! Tests for `lower::lower_expr` and `lower::lower_endpoint`.
//!
//! Spans correspond to the underlying `syn` node spans (Ident::span(),
//! Expr::span()); compound nodes inherit the span of their leftmost leaf.

use oscen_graph_compiler::ast::{BinaryOp, ConnectionExpr};
use oscen_graph_compiler::ir::expr::{IrExpr, IrExprKind};
use oscen_graph_compiler::ir::graph::{IrGraph, IrNode, IrNodeKind, NodeId};
use oscen_graph_compiler::ir::lower;
use proc_macro2::Span;
use quote::format_ident;
use std::collections::HashMap;
use syn::parse_quote;

fn mk_graph_with_node(name: &str) -> (IrGraph, HashMap<String, NodeId>, NodeId) {
    let mut ir = IrGraph::new(format_ident!("G"), false);
    let id = ir.nodes.insert_with_key(|id| IrNode {
        id,
        kind: IrNodeKind::Processor {
            ty: Some(parse_quote!(Dummy)),
            ctor_expr: parse_quote!(Dummy {}),
        },
        name: format_ident!("{}", name),
        rate: oscen_graph_compiler::ast::NodeRate::Same,
        latency_samples: 0,
        span: Span::call_site(),
        endpoints: Default::default(),
        incoming: Vec::new(),
        outgoing: Vec::new(),
    });
    let mut map = HashMap::new();
    map.insert(name.to_string(), id);
    (ir, map, id)
}

#[test]
fn lower_ident_resolves_to_endpoint() {
    let (ir, map, id) = mk_graph_with_node("cutoff");
    let ast = ConnectionExpr::Ident(format_ident!("cutoff"));
    let result = lower::lower_expr(&ast, &map, &ir).expect("lower succeeds");
    match result.kind {
        IrExprKind::Endpoint(ep) => {
            assert_eq!(ep.node, id);
            assert_eq!(ep.endpoint.to_string(), "cutoff");
            assert_eq!(ep.index, None);
        }
        other => panic!("expected Endpoint, got {:?}", other),
    }
}

#[test]
fn lower_field_access_resolves_to_endpoint() {
    let (ir, map, id) = mk_graph_with_node("osc");
    let ast = ConnectionExpr::Field(
        Box::new(ConnectionExpr::Ident(format_ident!("osc"))),
        format_ident!("output"),
    );
    let result = lower::lower_expr(&ast, &map, &ir).expect("lower succeeds");
    match result.kind {
        IrExprKind::Endpoint(ep) => {
            assert_eq!(ep.node, id);
            assert_eq!(ep.endpoint.to_string(), "output");
            assert_eq!(ep.index, None);
        }
        other => panic!("expected Endpoint, got {:?}", other),
    }
}

#[test]
fn lower_array_index_field_resolves_to_indexed_endpoint() {
    let (ir, map, id) = mk_graph_with_node("voices");
    let ast = ConnectionExpr::Field(
        Box::new(ConnectionExpr::ArrayIndex(
            Box::new(ConnectionExpr::Ident(format_ident!("voices"))),
            3,
        )),
        format_ident!("output"),
    );
    let result = lower::lower_expr(&ast, &map, &ir).expect("lower succeeds");
    match result.kind {
        IrExprKind::Endpoint(ep) => {
            assert_eq!(ep.node, id);
            assert_eq!(ep.endpoint.to_string(), "output");
            assert_eq!(ep.index, Some(3));
        }
        other => panic!("expected Endpoint, got {:?}", other),
    }
}

#[test]
fn lower_binary_recurses_on_both_sides() {
    let (mut ir, mut map, _) = mk_graph_with_node("a");
    // Add a second node "b"
    let b_id = ir.nodes.insert_with_key(|id| IrNode {
        id,
        kind: IrNodeKind::Processor {
            ty: Some(parse_quote!(Dummy)),
            ctor_expr: parse_quote!(Dummy {}),
        },
        name: format_ident!("b"),
        rate: oscen_graph_compiler::ast::NodeRate::Same,
        latency_samples: 0,
        span: Span::call_site(),
        endpoints: Default::default(),
        incoming: Vec::new(),
        outgoing: Vec::new(),
    });
    map.insert("b".to_string(), b_id);

    let ast = ConnectionExpr::Binary(
        Box::new(ConnectionExpr::Ident(format_ident!("a"))),
        BinaryOp::Mul,
        Box::new(ConnectionExpr::Ident(format_ident!("b"))),
    );
    let result = lower::lower_expr(&ast, &map, &ir).expect("lower succeeds");
    match result.kind {
        IrExprKind::Binary { left, op, right } => {
            assert!(matches!(op, BinaryOp::Mul));
            assert!(matches!(left.kind, IrExprKind::Endpoint(_)));
            assert!(matches!(right.kind, IrExprKind::Endpoint(_)));
        }
        other => panic!("expected Binary, got {:?}", other),
    }
}

#[test]
fn lower_literal_passes_through() {
    let (ir, map, _) = mk_graph_with_node("x");
    let ast = ConnectionExpr::Literal(parse_quote!(0.5));
    let result = lower::lower_expr(&ast, &map, &ir).expect("lower succeeds");
    assert!(matches!(result.kind, IrExprKind::Literal(_)));
}

#[test]
fn lower_unknown_ident_returns_none() {
    let (ir, map, _) = mk_graph_with_node("x");
    let ast = ConnectionExpr::Ident(format_ident!("nonexistent"));
    assert!(lower::lower_expr(&ast, &map, &ir).is_none());
}

#[test]
fn lower_endpoint_rejects_compound_expression() {
    let (ir, map, _) = mk_graph_with_node("x");
    let ast = ConnectionExpr::Binary(
        Box::new(ConnectionExpr::Ident(format_ident!("x"))),
        BinaryOp::Mul,
        Box::new(ConnectionExpr::Literal(parse_quote!(2.0))),
    );
    assert!(lower::lower_endpoint(&ast, &map, &ir).is_none());
}
