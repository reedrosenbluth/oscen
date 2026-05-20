//! AST → IR lowering.
//!
//! `lower(graph_def, diags)` walks the eight analysis steps in order
//! and populates an `IrGraph`. Steps are private free functions —
//! they're one-shot lowering helpers, not part of the IR's public
//! mutation API. Accumulates diagnostics across all steps and returns
//! `None` if any errors landed.

use crate::ast::{ConnectionExpr, ConnectionPolicy, EndpointKind, GraphDef, GraphItem, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::ir::graph::{
    classify_fanout, EdgeId, EdgeKernel, EndpointInfo, EndpointRef, EventRescale, FanoutShape,
    IrEdge, IrGraph, IrNode, IrNodeKind, NodeId,
};
use proc_macro2::Span;
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
    let nih_params = graph_def
        .items
        .iter()
        .any(|i| matches!(i, GraphItem::NihParams));
    let mut ir = IrGraph::new(name, nih_params);
    let mut name_to_id: HashMap<String, NodeId> = HashMap::new();

    collect_declarations(&graph_def, &mut ir, &mut name_to_id, diags);

    // Validate per-node rates BEFORE building edges so that the diagnostic
    // order matches the legacy `analyze()` -> `validate_connections()`
    // pipeline (rate errors come first, then type-mismatch errors).
    validate_node_rates(&ir, diags);

    infer_endpoint_types(&graph_def, &mut ir, &name_to_id, diags);
    build_edges(&graph_def, &mut ir, &name_to_id, diags);
    analyze_rates(&mut ir, diags);
    refine_kernels(&mut ir);
    topo_sort(&mut ir, diags);
    validate_cross_rate_kinds(&ir, diags);

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
                    kind: IrNodeKind::Input {
                        spec: input.spec.clone(),
                        default: input.default.clone(),
                    },
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
            ctor_expr: decl.constructor.clone(),
            len,
        }
    } else {
        IrNodeKind::Processor {
            ty: decl.node_type.clone(),
            ctor_expr: decl.constructor.clone(),
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
                if let Some((dst_id, dst_ep)) = resolve_node_endpoint(&stmt.dest, name_to_id) {
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
                if let Some((src_id, src_ep)) = resolve_node_endpoint(&stmt.source, name_to_id) {
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
        // Resolve source EndpointRef. For compound sources (binary, calls,
        // literals) fall back to the leftmost root node with a synthetic
        // endpoint name so the edge still exists in the IR — codegen reads
        // the full shape from `source_expr`, and dead-node analysis uses
        // `extra_source_nodes` to cover the other referenced idents.
        let (src_ref, extra_sources) = match resolve_endpoint_ref(&stmt.source, name_to_id) {
            Some(r) => (r, Vec::new()),
            None => {
                // Collect every node id referenced by the compound source.
                let mut refs = collect_referenced_node_ids(&stmt.source, name_to_id);
                refs.dedup();
                if refs.is_empty() {
                    // No node references at all (pure literal source). Skip
                    // — we can't anchor this edge to a node.
                    continue;
                }
                let primary = refs[0];
                let extras = refs[1..].to_vec();
                let ep_name = ir.nodes[primary].name.clone();
                (
                    EndpointRef {
                        node: primary,
                        endpoint: ep_name,
                    },
                    extras,
                )
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

        // Lower to IR forms. We use the AST source/dest to construct typed IR
        // representations alongside the legacy AST fields (parallel-path).
        let ir_source = match lower_expr(&stmt.source, name_to_id, ir) {
            Some(e) => e,
            None => {
                // Source doesn't resolve — but if src_ref already resolved
                // (compound source fallback), construct a synthetic IrExpr
                // pointing at the primary node so the edge still gets an IR
                // representation. The validator only checks the leaf node IDs
                // agree, which they will.
                crate::ir::expr::IrExpr {
                    kind: crate::ir::expr::IrExprKind::Endpoint(crate::ir::expr::IrEndpoint {
                        node: src_ref.node,
                        endpoint: src_ref.endpoint.clone(),
                        index: None,
                        span: stmt.span,
                    }),
                    span: stmt.span,
                }
            }
        };
        let ir_dest = match lower_endpoint(&stmt.dest, name_to_id, ir) {
            Some(d) => d,
            None => {
                // Same fallback for compound dests (shouldn't happen since
                // resolve_endpoint_ref already returned dst_ref).
                crate::ir::expr::IrEndpoint {
                    node: dst_ref.node,
                    endpoint: dst_ref.endpoint.clone(),
                    index: None,
                    span: stmt.span,
                }
            }
        };

        // Insert the edge.
        let src_ref_clone = src_ref.clone();
        let dst_ref_clone = dst_ref.clone();
        let extras_clone = extra_sources.clone();
        let eid = ir.edges.insert_with_key(|id| IrEdge {
            id,
            source: src_ref_clone,
            dest: dst_ref_clone,
            policy: stmt.policy,
            kernel: EdgeKernel::None,
            fanout: FanoutShape::Scalar,
            span: stmt.span,
            source_expr: stmt.source.clone(),
            dest_expr: stmt.dest.clone(),
            extra_source_nodes: extras_clone,
            ir_source,
            ir_dest,
        });

        // Update adjacency and canonical edge order.
        ir.nodes[src_ref.node].outgoing.push(eid);
        for &extra in &extra_sources {
            ir.nodes[extra].outgoing.push(eid);
        }
        ir.nodes[dst_ref.node].incoming.push(eid);
        ir.edge_order.push(eid);
    }
}

/// Collect every `NodeId` referenced by an expression in left-to-right
/// traversal order. Used by `build_edges` to anchor edges whose source is a
/// compound expression (binary, method call, free call). The first id is
/// promoted to `IrEdge::source.node`; the rest are stored in
/// `extra_source_nodes` so dead-node analysis can keep them all alive.
fn collect_referenced_node_ids(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_referenced_node_ids_into(expr, name_to_id, &mut out);
    out
}

fn collect_referenced_node_ids_into(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
    out: &mut Vec<NodeId>,
) {
    match expr {
        ConnectionExpr::Ident(i) => {
            if let Some(&id) = name_to_id.get(&i.to_string()) {
                out.push(id);
            }
        }
        ConnectionExpr::Field(base, _) => collect_referenced_node_ids_into(base, name_to_id, out),
        ConnectionExpr::ArrayIndex(base, _) => {
            collect_referenced_node_ids_into(base, name_to_id, out)
        }
        ConnectionExpr::MethodCall(base, _, _) => {
            collect_referenced_node_ids_into(base, name_to_id, out)
        }
        ConnectionExpr::Binary(l, _, r) => {
            collect_referenced_node_ids_into(l, name_to_id, out);
            collect_referenced_node_ids_into(r, name_to_id, out);
        }
        ConnectionExpr::Call(_, args) => {
            for a in args {
                collect_referenced_node_ids_into(a, name_to_id, out);
            }
        }
        ConnectionExpr::Literal(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Step 4: Rate analysis
// ---------------------------------------------------------------------------

/// Step 4: Classify each edge's resampling kernel and fanout shape.
///
/// For each edge, compares the source node's rate and the dest node's rate,
/// selects the appropriate `EdgeKernel` (None / Up / Down), and computes the
/// `FanoutShape` from node array sizes. Ports `rate_analysis::analyze` to
/// operate on an already-populated `IrGraph` instead of the AST.
///
/// Any invalid rate combination (e.g., two differently-rated non-default-rate
/// nodes) is pushed to `diags` without bailing. `EdgeKernel::None` is used
/// as a placeholder on errored edges.
fn analyze_rates(ir: &mut IrGraph, diags: &mut Diagnostics) {
    // Collect edge IDs up front to avoid borrow conflicts when mutating.
    let edge_ids: Vec<_> = ir.edges.keys().collect();

    for eid in edge_ids {
        let (src_node_id, dst_node_id, policy, span) = {
            let edge = &ir.edges[eid];
            (edge.source.node, edge.dest.node, edge.policy, edge.span)
        };

        let source_rate = ir.nodes[src_node_id].rate;
        let dest_rate = ir.nodes[dst_node_id].rate;

        // Reject undersampling (mirrors rate_analysis::analyze).
        if let NodeRate::Down(_) = source_rate {
            diags.push_error(syn::Error::new(
                ir.nodes[src_node_id].span,
                "node undersampling (`/ N`) is not yet supported in v1; only oversampling (`* N`) is implemented",
            ));
            // Leave kernel as None.
            continue;
        }
        if let NodeRate::Down(_) = dest_rate {
            diags.push_error(syn::Error::new(
                ir.nodes[dst_node_id].span,
                "node undersampling (`/ N`) is not yet supported in v1; only oversampling (`* N`) is implemented",
            ));
            continue;
        }

        let kernel = match classify_edge_ir(source_rate, dest_rate, policy, span) {
            Ok(k) => k,
            Err(e) => {
                diags.push_error(e);
                EdgeKernel::None
            }
        };

        // Compute fanout shape from source/dest node array sizes.
        let src_array_size = array_size_of(&ir.nodes[src_node_id].kind);
        let dst_array_size = array_size_of(&ir.nodes[dst_node_id].kind);
        let fanout = classify_fanout(src_array_size, dst_array_size);

        ir.edges[eid].kernel = kernel;
        ir.edges[eid].fanout = fanout;
    }
}

/// Per-node rate validation. Catches `Down(n)` rate annotations
/// (currently unsupported) even on nodes with no edges. Mirrors the
/// per-node check in `rate_analysis::analyze` so that unconnected
/// undersampled nodes also produce a diagnostic.
fn validate_node_rates(ir: &IrGraph, diags: &mut Diagnostics) {
    for &id in &ir.processors {
        let node = &ir.nodes[id];
        if let NodeRate::Down(n) = node.rate {
            if n > 1 {
                diags.push_error(syn::Error::new(
                    node.span,
                    "node undersampling (`/ N`) is not yet supported in v1; only oversampling (`* N`) is implemented",
                ));
            }
        }
    }
}

/// Step 5: Refine edge kernels using endpoint-kind information.
///
/// Two refinements mirror `rate_analysis::refine_with_types`:
///
/// 1. **Event edges.** Any edge whose source or destination endpoint is an
///    event endpoint is rewritten to `EdgeKernel::Event` with rescaling
///    derived from the source/dest rates.
///
/// 2. **Default policy on value cross-rate edges** is promoted to
///    `ConnectionPolicy::Latch`. Stream edges keep their Sinc default.
///
/// No diagnostics are emitted here; bad kind tuples are caught by
/// `validate_cross_rate_kinds` (step 8, Task 9).
fn refine_kernels(ir: &mut IrGraph) {
    let edge_ids: Vec<_> = ir.edges.keys().collect();

    for eid in edge_ids {
        let (src_node_id, dst_node_id, src_ep, dst_ep) = {
            let edge = &ir.edges[eid];
            (
                edge.source.node,
                edge.dest.node,
                edge.source.endpoint.clone(),
                edge.dest.endpoint.clone(),
            )
        };

        let src_kind = ir.nodes[src_node_id].endpoints.get(&src_ep).map(|e| e.kind);
        let dst_kind = ir.nodes[dst_node_id].endpoints.get(&dst_ep).map(|e| e.kind);

        let is_event_edge = matches!(src_kind, Some(EndpointKind::Event))
            || matches!(dst_kind, Some(EndpointKind::Event));

        if is_event_edge {
            let source_rate = ir.nodes[src_node_id].rate;
            let dest_rate = ir.nodes[dst_node_id].rate;
            let rescale = compute_event_rescale(source_rate, dest_rate);
            ir.edges[eid].kernel = EdgeKernel::Event { rescale };
            continue;
        }

        // Promote Default policy to Latch on value cross-rate edges.
        let is_value_edge = matches!(src_kind, Some(EndpointKind::Value))
            || matches!(dst_kind, Some(EndpointKind::Value));
        if is_value_edge {
            match &mut ir.edges[eid].kernel {
                EdgeKernel::Up { kind, .. } | EdgeKernel::Down { kind, .. } => {
                    if matches!(kind, ConnectionPolicy::Default) {
                        *kind = ConnectionPolicy::Latch;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Classify a cross-rate edge, mirroring `rate_analysis::classify_edge`.
fn classify_edge_ir(
    src: NodeRate,
    dst: NodeRate,
    policy: ConnectionPolicy,
    span: proc_macro2::Span,
) -> syn::Result<EdgeKernel> {
    use NodeRate::*;
    let (factor, is_up) = match (src, dst) {
        (Same, Same) => return Ok(EdgeKernel::None),
        (Up(n), Same) => (n, false), // source faster → downsample at dest
        (Same, Up(n)) => (n, true),  // dest faster → upsample from source
        (Same, Down(n)) => (n, false),
        (Down(n), Same) => (n, true),
        (Up(a), Up(b)) if a == b => return Ok(EdgeKernel::None),
        (Down(a), Down(b)) if a == b => return Ok(EdgeKernel::None),
        _ => {
            return Err(syn::Error::new(
                span,
                "v1 does not support connections between two differently-rated non-default-rate nodes; \
                 route through an outer-rate node instead",
            ));
        }
    };

    Ok(if is_up {
        EdgeKernel::Up {
            factor,
            kind: policy,
        }
    } else {
        EdgeKernel::Down {
            factor,
            kind: policy,
        }
    })
}

/// Compute the `EventRescale` for an event edge given source/dest rates.
/// Mirrors `rate_analysis::event_rescale`.
fn compute_event_rescale(src: NodeRate, dst: NodeRate) -> EventRescale {
    use NodeRate::*;
    match (src, dst) {
        (Same, Up(n)) => EventRescale::Multiply(n),
        (Up(n), Same) => EventRescale::Divide(n),
        _ => EventRescale::None,
    }
}

/// Extract the array size from an `IrNodeKind`, if it is a `NodeArray`.
fn array_size_of(kind: &IrNodeKind) -> Option<usize> {
    match kind {
        IrNodeKind::NodeArray { len, .. } => Some(*len),
        _ => None,
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
///
/// This is the single source of truth for endpoint-kind inference. Both the
/// lowering pass and `codegen::CodegenContext::infer_kind` delegate here so
/// cross-rate kernel projection and policy promotion rules stay in sync.
pub(crate) fn endpoint_kind_of(
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
fn root_node_id(expr: &ConnectionExpr, name_to_id: &HashMap<String, NodeId>) -> Option<NodeId> {
    match expr {
        ConnectionExpr::Ident(ident) => name_to_id.get(&ident.to_string()).copied(),
        ConnectionExpr::Field(obj, _) => root_node_id(obj, name_to_id),
        ConnectionExpr::MethodCall(obj, _, _) => root_node_id(obj, name_to_id),
        ConnectionExpr::ArrayIndex(inner, _) => root_node_id(inner, name_to_id),
        ConnectionExpr::Binary(_, _, _)
        | ConnectionExpr::Literal(_)
        | ConnectionExpr::Call(_, _) => None,
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

// ---------------------------------------------------------------------------
// Step 6: Topological sort
// ---------------------------------------------------------------------------

/// Step 6: Sort `ir.processors` into topological (dependency) order using
/// Kahn's algorithm.
///
/// Feedback-allowing nodes (identified by `is_feedback_allowing_node`) have
/// their incoming edges excluded from the in-degree count so they don't
/// create false cycles. Emits a "non-feedback cycle" error into `diags` if
/// the graph is cyclic after removing feedback edges.
fn topo_sort(ir: &mut IrGraph, diags: &mut Diagnostics) {
    use std::collections::{HashMap, VecDeque};

    // Build a set of processor NodeIds for fast membership test.
    let processor_set: std::collections::HashSet<NodeId> = ir.processors.iter().copied().collect();

    // Compute in-degree for each processor from edges whose source is also
    // a processor, skipping edges from feedback-allowing sources.
    let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
    for &nid in &ir.processors {
        in_degree.insert(nid, 0);
    }
    for &nid in &ir.processors {
        for &eid in &ir.nodes[nid].incoming {
            let edge = &ir.edges[eid];
            // Primary source contributes if it's a processor and isn't a
            // feedback-allowing node. Compound-source edges additionally
            // count every `extra_source_nodes` entry that meets the same
            // criteria — those are the other nodes the source expression
            // reads (e.g., `a.x * b.y -> dst` has b as an extra source).
            let mut count_src = |src: NodeId| {
                if processor_set.contains(&src) && !is_feedback_allowing_node(&ir.nodes[src]) {
                    *in_degree.get_mut(&nid).unwrap() += 1;
                }
            };
            count_src(edge.source.node);
            for &extra in &edge.extra_source_nodes {
                count_src(extra);
            }
        }
    }

    // Seed the queue with all zero-in-degree nodes.
    let mut queue: VecDeque<NodeId> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut sorted: Vec<NodeId> = Vec::with_capacity(ir.processors.len());

    while let Some(nid) = queue.pop_front() {
        sorted.push(nid);
        // Edges OUT of feedback-allowing nodes don't impose ordering — they
        // were skipped during in-degree calculation, so we must also skip
        // them here to keep the algorithm symmetric.
        if is_feedback_allowing_node(&ir.nodes[nid]) {
            continue;
        }
        // Clone outgoing to avoid borrow conflict when mutating in_degree.
        let outgoing: Vec<EdgeId> = ir.nodes[nid].outgoing.clone();
        for eid in outgoing {
            let dst = ir.edges[eid].dest.node;
            if let Some(d) = in_degree.get_mut(&dst) {
                if *d > 0 {
                    *d -= 1;
                    if *d == 0 {
                        queue.push_back(dst);
                    }
                }
            }
        }
    }

    if sorted.len() != ir.processors.len() {
        diags.push_error(syn::Error::new(
            proc_macro2::Span::call_site(),
            "graph contains a non-feedback cycle",
        ));
        return;
    }
    ir.processors = sorted;
}

/// Return `true` if `node` is a feedback-allowing node (e.g., a delay line).
///
/// Uses the same string-match heuristic as `codegen::is_feedback_allowing_node`
/// (line ~887): the last path segment of the node's type must contain "Delay".
/// Replacing this with a marker-trait approach is tracked separately.
fn is_feedback_allowing_node(node: &IrNode) -> bool {
    match &node.kind {
        IrNodeKind::Processor { ty: Some(path), .. }
        | IrNodeKind::NodeArray { ty: Some(path), .. } => {
            if let Some(seg) = path.segments.last() {
                return seg.ident.to_string().contains("Delay");
            }
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Step 8: Cross-rate kind validation
// ---------------------------------------------------------------------------

/// Step 8: Walk all cross-rate edges and push diagnostics for unsupported
/// (src kind, dst kind) tuples.
///
/// Ports `rate_analysis::validate_cross_rate_kinds` to operate on `&IrGraph`
/// instead of the legacy `RateAnalysis`/`TypeContext` side-tables. Edges
/// where one or both endpoint kinds cannot be inferred are skipped — those
/// produce errors elsewhere. Does not bail on the first error; all bad edges
/// are reported.
fn validate_cross_rate_kinds(ir: &IrGraph, diags: &mut Diagnostics) {
    for edge in ir.edges.values() {
        let is_cross_rate = matches!(edge.kernel, EdgeKernel::Up { .. } | EdgeKernel::Down { .. });
        if !is_cross_rate {
            continue;
        }

        let src_kind = ir.nodes[edge.source.node]
            .endpoints
            .get(&edge.source.endpoint)
            .map(|e| e.kind);
        let dst_kind = ir.nodes[edge.dest.node]
            .endpoints
            .get(&edge.dest.endpoint)
            .map(|e| e.kind);

        let (src, dst) = match (src_kind, dst_kind) {
            (Some(s), Some(d)) => (s, d),
            _ => continue,
        };

        if is_supported_cross_rate_kinds(src, dst) {
            continue;
        }

        diags.push_error(syn::Error::new(
            edge.span,
            format!(
                "cross-rate edge from {} to {} is not supported; \
                 insert an explicit converter node, or change one side's rate",
                endpoint_kind_name(src),
                endpoint_kind_name(dst),
            ),
        ));
    }
}

/// Cross-rate edges support a fixed set of `(SrcKind, DstKind)` tuples.
/// Mirrors `rate_analysis::is_supported_cross_rate_kinds` exactly.
fn is_supported_cross_rate_kinds(src: EndpointKind, dst: EndpointKind) -> bool {
    matches!(
        (src, dst),
        (EndpointKind::Stream, EndpointKind::Stream)
            | (EndpointKind::Value, EndpointKind::Value)
            | (EndpointKind::Value, EndpointKind::Stream)
            | (EndpointKind::Event, EndpointKind::Event)
    )
}

fn endpoint_kind_name(kind: EndpointKind) -> &'static str {
    match kind {
        EndpointKind::Stream => "stream",
        EndpointKind::Value => "value",
        EndpointKind::Event => "event",
    }
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

// ---------------------------------------------------------------------------
// Public lowering API: AST ConnectionExpr → typed IrExpr / IrEndpoint
// ---------------------------------------------------------------------------

use crate::ir::expr::{IrEndpoint, IrExpr, IrExprKind};

/// Convert an AST `ConnectionExpr` into a typed `IrExpr` with all endpoint
/// references resolved against `name_to_id`.
///
/// Returns `None` if any referenced ident doesn't resolve to a known node.
/// Callers are responsible for pushing diagnostics on failure.
///
/// Spans on the resulting `IrExpr` nodes are derived from the underlying
/// `syn` nodes: `Ident::span()` for endpoint refs, `Expr::span()` for
/// literals and method-call args. Compound nodes (`Binary`, `MethodCall`,
/// `Call`) inherit the span of their leftmost leaf.
pub fn lower_expr(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
    ir: &IrGraph,
) -> Option<IrExpr> {
    match expr {
        ConnectionExpr::Ident(ident) => {
            let id = *name_to_id.get(&ident.to_string())?;
            Some(IrExpr {
                kind: IrExprKind::Endpoint(IrEndpoint {
                    node: id,
                    endpoint: ident.clone(),
                    index: None,
                    span: ident.span(),
                }),
                span: ident.span(),
            })
        }
        ConnectionExpr::Field(obj, field) => {
            let (node, index, anchor_span) = resolve_field_base(obj, name_to_id)?;
            Some(IrExpr {
                kind: IrExprKind::Endpoint(IrEndpoint {
                    node,
                    endpoint: field.clone(),
                    index,
                    span: field.span(),
                }),
                span: anchor_span,
            })
        }
        ConnectionExpr::ArrayIndex(inner, idx) => {
            let inner_expr = lower_expr(inner, name_to_id, ir)?;
            if let IrExprKind::Endpoint(IrEndpoint {
                node,
                endpoint,
                index: None,
                span,
            }) = inner_expr.kind
            {
                Some(IrExpr {
                    kind: IrExprKind::Endpoint(IrEndpoint {
                        node,
                        endpoint,
                        index: Some(*idx),
                        span,
                    }),
                    span: inner_expr.span,
                })
            } else {
                None
            }
        }
        ConnectionExpr::Binary(left, op, right) => {
            let lhs = lower_expr(left, name_to_id, ir)?;
            let rhs = lower_expr(right, name_to_id, ir)?;
            let span = lhs.span;
            Some(IrExpr {
                kind: IrExprKind::Binary {
                    left: Box::new(lhs),
                    op: *op,
                    right: Box::new(rhs),
                },
                span,
            })
        }
        ConnectionExpr::MethodCall(receiver, method, args) => {
            let recv = lower_expr(receiver, name_to_id, ir)?;
            let span = recv.span;
            Some(IrExpr {
                kind: IrExprKind::MethodCall {
                    receiver: Box::new(recv),
                    method: method.clone(),
                    args: args.clone(),
                },
                span,
            })
        }
        ConnectionExpr::Call(func, args) => {
            let ir_args: Option<Vec<_>> = args
                .iter()
                .map(|a| lower_expr(a, name_to_id, ir))
                .collect();
            let ir_args = ir_args?;
            Some(IrExpr {
                kind: IrExprKind::Call {
                    function: func.clone(),
                    args: ir_args,
                },
                span: func.span(),
            })
        }
        ConnectionExpr::Literal(lit) => {
            use syn::spanned::Spanned;
            let span = lit.span();
            Some(IrExpr {
                kind: IrExprKind::Literal(lit.clone()),
                span,
            })
        }
    }
}

/// Resolve a Field base (`Ident` or `ArrayIndex(Ident, idx)`) to its
/// `(node, optional index, anchor span)`.
fn resolve_field_base(
    base: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
) -> Option<(NodeId, Option<usize>, Span)> {
    match base {
        ConnectionExpr::Ident(i) => {
            let id = *name_to_id.get(&i.to_string())?;
            Some((id, None, i.span()))
        }
        ConnectionExpr::ArrayIndex(inner, idx) => {
            if let ConnectionExpr::Ident(i) = inner.as_ref() {
                let id = *name_to_id.get(&i.to_string())?;
                Some((id, Some(*idx), i.span()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Convert an AST `ConnectionExpr` representing a destination (must be
/// addressable: `out`, `node.field`, or `voices[k].field`) into an
/// `IrEndpoint`. Returns `None` for any expression shape that isn't
/// addressable (`Binary`, `Call`, `Literal`, etc.).
pub fn lower_endpoint(
    expr: &ConnectionExpr,
    name_to_id: &HashMap<String, NodeId>,
    _ir: &IrGraph,
) -> Option<IrEndpoint> {
    match expr {
        ConnectionExpr::Ident(ident) => {
            let id = *name_to_id.get(&ident.to_string())?;
            Some(IrEndpoint {
                node: id,
                endpoint: ident.clone(),
                index: None,
                span: ident.span(),
            })
        }
        ConnectionExpr::Field(obj, field) => {
            let (node, index, _anchor) = resolve_field_base(obj, name_to_id)?;
            Some(IrEndpoint {
                node,
                endpoint: field.clone(),
                index,
                span: field.span(),
            })
        }
        _ => None,
    }
}
