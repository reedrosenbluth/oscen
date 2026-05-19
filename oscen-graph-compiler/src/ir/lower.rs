//! AST → IR lowering.
//!
//! `lower(graph_def, diags)` walks the eight analysis steps in order
//! and populates an `IrGraph`. Steps are private free functions —
//! they're one-shot lowering helpers, not part of the IR's public
//! mutation API. Accumulates diagnostics across all steps and returns
//! `None` if any errors landed.

use crate::ast::{ConnectionExpr, EndpointKind, GraphDef, GraphItem, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::fanout::FanoutShape;
use crate::ir::graph::{EndpointInfo, EndpointRef, IrEdge, IrGraph, IrNode, IrNodeKind, NodeId};
use crate::rate_analysis::EdgeKernel;
use proc_macro2::Span;
use quote::ToTokens;
use std::collections::HashMap;
use syn::Ident;

pub fn lower(graph_def: GraphDef, diags: &mut Diagnostics) -> Option<IrGraph> {
    let name = match graph_def.name.clone() {
        Some(n) => n,
        None => {
            diags.push_error(syn::Error::new(
                Span::call_site(),
                "graph! macro requires a name (anonymous graphs are no longer supported)",
            ));
            return None;
        }
    };
    let nih_params = graph_def.items.iter().any(|i| matches!(i, GraphItem::NihParams));
    let mut ir = IrGraph::new(name, nih_params);
    let mut name_to_id: HashMap<String, NodeId> = HashMap::new();

    collect_declarations(&graph_def, &mut ir, &mut name_to_id, diags);

    infer_endpoint_types(&graph_def, &mut ir, &name_to_id, diags);
    build_edges(&graph_def, &mut ir, &name_to_id, diags);
    // Steps 4–8 land in later tasks.

    #[cfg(debug_assertions)]
    crate::ir::validate::validate(&ir);

    if diags.is_empty() {
        Some(ir)
    } else {
        None
    }
}

/// Step 1: Walk `graph_def.items`, create `IrNode`s for inputs, outputs,
/// processors, and node arrays. Populates `name_to_id` for later steps
/// to resolve endpoint references.
fn collect_declarations(
    graph_def: &GraphDef,
    ir: &mut IrGraph,
    name_to_id: &mut HashMap<String, NodeId>,
    diags: &mut Diagnostics,
) {
    for item in &graph_def.items {
        match item {
            GraphItem::Input(input) => {
                let id = ir.nodes.insert_with_key(|id| IrNode {
                    id,
                    kind: IrNodeKind::Input { spec: input.spec.clone() },
                    name: input.name.clone(),
                    rate: NodeRate::Same,
                    latency_samples: 0,
                    span: input.name.span(),
                    endpoints: input_endpoints(input),
                    incoming: Vec::new(),
                    outgoing: Vec::new(),
                });
                ir.inputs.push(id);
                if name_to_id.insert(input.name.to_string(), id).is_some() {
                    diags.push_error(syn::Error::new(
                        input.name.span(),
                        format!("duplicate declaration of `{}`", input.name),
                    ));
                }
            }
            GraphItem::Output(output) => {
                let id = ir.nodes.insert_with_key(|id| IrNode {
                    id,
                    kind: IrNodeKind::Output,
                    name: output.name.clone(),
                    rate: NodeRate::Same,
                    latency_samples: 0,
                    span: output.name.span(),
                    endpoints: output_endpoints(output),
                    incoming: Vec::new(),
                    outgoing: Vec::new(),
                });
                ir.outputs.push(id);
                if name_to_id.insert(output.name.to_string(), id).is_some() {
                    diags.push_error(syn::Error::new(
                        output.name.span(),
                        format!("duplicate declaration of `{}`", output.name),
                    ));
                }
            }
            GraphItem::Node(node) => {
                collect_node_decl(node, ir, name_to_id, diags);
            }
            GraphItem::NodeBlock(block) => {
                for n in &block.0 {
                    collect_node_decl(n, ir, name_to_id, diags);
                }
            }
            // Connections + nih_params + name don't create IrNodes;
            // they're handled by later lowering steps.
            GraphItem::Connection(_)
            | GraphItem::ConnectionBlock(_)
            | GraphItem::NihParams
            | GraphItem::Name(_) => {}
        }
    }
}

fn collect_node_decl(
    decl: &crate::ast::NodeDecl,
    ir: &mut IrGraph,
    name_to_id: &mut HashMap<String, NodeId>,
    diags: &mut Diagnostics,
) {
    // NodeArray vs Processor classification: `array_size: Some(n)` → NodeArray.
    let kind = if let Some(len) = decl.array_size {
        IrNodeKind::NodeArray {
            ty: decl.node_type.clone(),
            ctor: decl.constructor.to_token_stream(),
            len,
        }
    } else {
        IrNodeKind::Processor {
            ty: decl.node_type.clone(),
            ctor: decl.constructor.to_token_stream(),
        }
    };
    let id = ir.nodes.insert_with_key(|id| IrNode {
        id,
        kind,
        name: decl.name.clone(),
        rate: decl.rate,
        latency_samples: 0,
        span: decl.name.span(),
        endpoints: HashMap::new(),
        incoming: Vec::new(),
        outgoing: Vec::new(),
    });
    ir.processors.push(id);
    if name_to_id.insert(decl.name.to_string(), id).is_some() {
        diags.push_error(syn::Error::new(
            decl.name.span(),
            format!("duplicate declaration of `{}`", decl.name),
        ));
    }
}

fn input_endpoints(input: &crate::ast::InputDecl) -> HashMap<Ident, EndpointInfo> {
    let mut m = HashMap::new();
    // The "name" of the implicit endpoint on an input/output decl is
    // the decl's own identifier — `s -> osc.frequency` references the
    // `s` endpoint on the `s` input node.
    m.insert(input.name.clone(), EndpointInfo { kind: input.kind });
    m
}

fn output_endpoints(output: &crate::ast::OutputDecl) -> HashMap<Ident, EndpointInfo> {
    let mut m = HashMap::new();
    m.insert(output.name.clone(), EndpointInfo { kind: output.kind });
    m
}

/// Step 2: Fixed-point inference of node-endpoint types from connection
/// shapes. Ports logic from `type_check::TypeContext::infer_type` plus
/// fixed-point iteration.
///
/// Strategy: when a connection has a known-typed source feeding a node
/// endpoint, the destination endpoint inherits that kind. Iterate until
/// no new types are inferred or the cap is reached.
fn infer_endpoint_types(
    graph_def: &GraphDef,
    ir: &mut IrGraph,
    name_to_id: &HashMap<String, NodeId>,
    _diags: &mut Diagnostics,
) {
    // Collect all connection statements from the graph def.
    let stmts: Vec<&crate::ast::ConnectionStmt> = graph_def
        .items
        .iter()
        .flat_map(|item| match item {
            GraphItem::Connection(c) => std::slice::from_ref(c),
            GraphItem::ConnectionBlock(b) => b.0.as_slice(),
            _ => &[],
        })
        .collect();

    let cap = stmts.len() + 1;
    for _ in 0..cap {
        let mut changed = false;

        for stmt in &stmts {
            // Infer source kind.
            let src_kind = endpoint_kind_of(&stmt.source, ir, name_to_id);

            // If the destination is a node.endpoint, propagate the kind.
            if let Some(src_kind) = src_kind {
                if let Some((dst_id, dst_ep)) =
                    resolve_node_endpoint(&stmt.dest, name_to_id)
                {
                    let node = &mut ir.nodes[dst_id];
                    use std::collections::hash_map::Entry;
                    match node.endpoints.entry(dst_ep) {
                        Entry::Vacant(e) => {
                            e.insert(EndpointInfo { kind: src_kind });
                            changed = true;
                        }
                        Entry::Occupied(_) => {}
                    }
                }
            }

            // Symmetrically: if source is a node.endpoint whose kind
            // is unknown, try to infer it from the dest.
            let dst_kind = endpoint_kind_of(&stmt.dest, ir, name_to_id);
            if let Some(dst_kind) = dst_kind {
                if let Some((src_id, src_ep)) =
                    resolve_node_endpoint(&stmt.source, name_to_id)
                {
                    let node = &mut ir.nodes[src_id];
                    use std::collections::hash_map::Entry;
                    match node.endpoints.entry(src_ep) {
                        Entry::Vacant(e) => {
                            e.insert(EndpointInfo { kind: dst_kind });
                            changed = true;
                        }
                        Entry::Occupied(_) => {}
                    }
                }
            }
        }

        if !changed {
            break;
        }
    }
}

/// Step 3: Construct one IrEdge per connection statement.
/// Validates type compatibility (source kind vs dest kind).
/// Pushes type-mismatch errors into diags WITHOUT bailing.
fn build_edges(
    graph_def: &GraphDef,
    ir: &mut IrGraph,
    name_to_id: &HashMap<String, NodeId>,
    diags: &mut Diagnostics,
) {
    let stmts: Vec<crate::ast::ConnectionStmt> = graph_def
        .items
        .iter()
        .flat_map(|item| match item {
            GraphItem::Connection(c) => vec![c.clone()],
            GraphItem::ConnectionBlock(b) => b.0.clone(),
            _ => vec![],
        })
        .collect();

    for stmt in stmts {
        // Resolve source EndpointRef.
        let src_ref = match resolve_endpoint_ref(&stmt.source, name_to_id) {
            Some(r) => r,
            None => {
                // Can't resolve source — skip; a later lowering step or
                // Rust's type system will catch it.
                continue;
            }
        };

        // Resolve dest EndpointRef.
        let dst_ref = match resolve_endpoint_ref(&stmt.dest, name_to_id) {
            Some(r) => r,
            None => {
                continue;
            }
        };

        // Type-compatibility check.
        let src_kind = endpoint_kind_of(&stmt.source, ir, name_to_id);
        let dst_kind = endpoint_kind_of(&stmt.dest, ir, name_to_id);

        if let (Some(src), Some(dst)) = (src_kind, dst_kind) {
            if !types_compatible(src, dst) {
                let msg = format!(
                    "Type mismatch in connection: source is {:?} but destination expects {:?}",
                    src, dst
                );
                diags.push_error(syn::Error::new(stmt.span, msg));
                // Skip creating the edge on type mismatch.
                continue;
            }
        }

        // Insert the edge.
        let src_ref_clone = src_ref.clone();
        let dst_ref_clone = dst_ref.clone();
        let eid = ir.edges.insert_with_key(|id| IrEdge {
            id,
            source: src_ref_clone,
            dest: dst_ref_clone,
            policy: stmt.policy,
            kernel: EdgeKernel::None,
            fanout: FanoutShape::Scalar,
            span: stmt.span,
            source_expr: stmt.source.clone(),
        });

        // Update adjacency.
        ir.nodes[src_ref.node].outgoing.push(eid);
        ir.nodes[dst_ref.node].incoming.push(eid);
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Infer the `EndpointKind` of an arbitrary `ConnectionExpr`, using the
/// node endpoint registry in `ir` for `Field` lookups and the `name_to_id`
/// map for `Ident` lookups against graph input/output nodes.
///
/// Ports `TypeContext::infer_type` from `type_check.rs`.
fn endpoint_kind_of(
    expr: &ConnectionExpr,
    ir: &IrGraph,
    name_to_id: &HashMap<String, NodeId>,
) -> Option<EndpointKind> {
    match expr {
        ConnectionExpr::Ident(ident) => {
            // Check if it's a known graph input or output node whose
            // implicit endpoint carries a kind.
            let id = name_to_id.get(&ident.to_string())?;
            let node = &ir.nodes[*id];
            // The implicit endpoint name on an input/output node is the
            // node's own name (see `input_endpoints` / `output_endpoints`).
            node.endpoints.get(ident).map(|ei| ei.kind)
        }
        ConnectionExpr::ArrayIndex(inner, _) => {
            // Array indexing preserves the type of the base expression.
            endpoint_kind_of(inner, ir, name_to_id)
        }
        ConnectionExpr::Field(obj, field) => {
            // Look up node.endpoint — the object must be a simple Ident.
            if let ConnectionExpr::Ident(node_ident) = &**obj {
                let id = name_to_id.get(&node_ident.to_string())?;
                let node = &ir.nodes[*id];
                node.endpoints.get(field).map(|ei| ei.kind)
            } else {
                None
            }
        }
        ConnectionExpr::MethodCall(_, _, _) => {
            // Method return types aren't known at this stage.
            None
        }
        ConnectionExpr::Binary(left, _op, right) => {
            let left_kind = endpoint_kind_of(left, ir, name_to_id)?;
            let right_kind = endpoint_kind_of(right, ir, name_to_id)?;
            match (left_kind, right_kind) {
                (EndpointKind::Stream, EndpointKind::Stream) => Some(EndpointKind::Stream),
                (EndpointKind::Stream, EndpointKind::Value) => Some(EndpointKind::Stream),
                (EndpointKind::Value, EndpointKind::Stream) => Some(EndpointKind::Stream),
                (EndpointKind::Value, EndpointKind::Value) => Some(EndpointKind::Value),
                (EndpointKind::Event, _) | (_, EndpointKind::Event) => None,
            }
        }
        ConnectionExpr::Literal(_) => {
            // Literals are treated as values.
            Some(EndpointKind::Value)
        }
        ConnectionExpr::Call(_, _) => {
            // Can't infer function return types without more context.
            None
        }
    }
}

/// Get the root `NodeId` from a complex expression.
/// For `osc.output[0]`, returns the id of `osc`.
/// Recurses through `Field`, `MethodCall`, `ArrayIndex`.
fn root_node_id(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
) -> Option<NodeId> {
    match expr {
        ConnectionExpr::Ident(ident) => name_to_id.get(&ident.to_string()).copied(),
        ConnectionExpr::Field(obj, _) => root_node_id(obj, name_to_id),
        ConnectionExpr::MethodCall(obj, _, _) => root_node_id(obj, name_to_id),
        ConnectionExpr::ArrayIndex(inner, _) => root_node_id(inner, name_to_id),
        ConnectionExpr::Binary(_, _, _) | ConnectionExpr::Literal(_) | ConnectionExpr::Call(_, _) => None,
    }
}

/// Extract the `(NodeId, endpoint Ident)` pair from a destination
/// expression like `osc.frequency` or a plain `out` (graph output).
/// Returns `None` for expressions that aren't addressable node endpoints.
fn resolve_node_endpoint(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
) -> Option<(NodeId, Ident)> {
    match expr {
        // Plain ident: must be a graph-level input or output node whose
        // implicit endpoint shares the node's name.
        ConnectionExpr::Ident(ident) => {
            let id = name_to_id.get(&ident.to_string())?;
            Some((*id, ident.clone()))
        }
        // `node.endpoint` — the most common case.
        ConnectionExpr::Field(obj, field) => {
            let id = root_node_id(obj, name_to_id)?;
            Some((id, field.clone()))
        }
        // Array index on a field: `voices[0].output` — the endpoint is the
        // field name, the node id is the root. We recurse on the inner expr.
        ConnectionExpr::ArrayIndex(inner, _) => resolve_node_endpoint(inner, name_to_id),
        _ => None,
    }
}

/// Resolve a `ConnectionExpr` to an `EndpointRef` (NodeId + endpoint Ident).
///
/// For plain idents (graph inputs/outputs), the endpoint name is the ident
/// itself — matching how `input_endpoints` / `output_endpoints` store them.
/// For field exprs (`osc.frequency`), the endpoint is the field name.
fn resolve_endpoint_ref(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
) -> Option<EndpointRef> {
    resolve_node_endpoint(expr, name_to_id).map(|(node, endpoint)| EndpointRef { node, endpoint })
}

/// Check whether a source kind is compatible with a destination kind.
///
/// Faithfully mirrors `TypeContext::validate_connection` in `type_check.rs`.
fn types_compatible(src: EndpointKind, dst: EndpointKind) -> bool {
    matches!(
        (src, dst),
        (EndpointKind::Stream, EndpointKind::Stream)
            | (EndpointKind::Value, EndpointKind::Value)
            | (EndpointKind::Event, EndpointKind::Event)
            | (EndpointKind::Value, EndpointKind::Stream)
    )
}
