//! AST → IR lowering.
//!
//! `lower(graph_def, diags)` walks the eight analysis steps in order
//! and populates an `IrGraph`. Steps are private free functions —
//! they're one-shot lowering helpers, not part of the IR's public
//! mutation API. Accumulates diagnostics across all steps and returns
//! `None` if any errors landed.

use crate::ast::{ConnectionExpr, ConnectionPolicy, EndpointKind, GraphDef, GraphItem, NodeRate};
use crate::diagnostics::Diagnostics;
use crate::ir::expr::{primary_node, IrEndpoint, IrExpr, IrExprKind};
use crate::ir::graph::{
    classify_fanout, EdgeId, EdgeKernel, EndpointInfo, EventRescale, FanoutShape, IrEdge, IrGraph,
    IrNode, IrNodeKind, NodeId,
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

/// Build the endpoint map for a synthesised `::oscen::Delay` node.
///
/// Mirrors the actual Delay endpoint descriptors from `oscen-lib/src/delay/mod.rs`:
/// - `input`         → Stream
/// - `output`        → Stream
/// - `delay_samples` → Value
/// - `feedback`      → Value
fn synth_delay_endpoints(span: proc_macro2::Span) -> HashMap<Ident, EndpointInfo> {
    use crate::ast::EndpointKind;
    let mut m = HashMap::new();
    m.insert(
        Ident::new("input", span),
        EndpointInfo {
            kind: EndpointKind::Stream,
        },
    );
    m.insert(
        Ident::new("output", span),
        EndpointInfo {
            kind: EndpointKind::Stream,
        },
    );
    m.insert(
        Ident::new("delay_samples", span),
        EndpointInfo {
            kind: EndpointKind::Value,
        },
    );
    m.insert(
        Ident::new("feedback", span),
        EndpointInfo {
            kind: EndpointKind::Value,
        },
    );
    m
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
            // Lower source/dest to IrExpr for type inference.
            // (Same call lower_expr will make again in build_edges; this is
            // unavoidable since infer_endpoint_types runs before edges exist.)
            let ir_source = lower_expr(&stmt.source, name_to_id, ir);
            let ir_dest_expr = lower_expr(&stmt.dest, name_to_id, ir);

            // Infer source kind.
            let src_kind = ir_source.as_ref().and_then(|e| endpoint_kind_of(e, ir));

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

            // Symmetric: if source is a node.endpoint whose kind is unknown,
            // try to infer it from the dest.
            let dst_kind = ir_dest_expr.as_ref().and_then(|e| endpoint_kind_of(e, ir));
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

/// Step 3: Construct one or two `IrEdge`s per connection statement.
///
/// - `src -> dst` (no via): one edge, `is_feedback: false`.
/// - `src -> [name] -> dst` (Node via): two edges through the declared node —
///   `src → via.input` (non-feedback) and `via.output → dst` (feedback).
/// - `src -> [N] -> dst` (Samples via): synthesises an anonymous `::oscen::Delay`
///   node with N samples, then emits two edges through it —
///   `src → synth.input` (non-feedback) and `synth.output → dst` (feedback).
///
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

    let mut synth_counter: u32 = 0;
    let mut via_used_nodes: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for stmt in stmts {
        // Lower to IR forms.
        let ir_source = match lower_expr(&stmt.source, name_to_id, ir) {
            Some(e) => e,
            None => continue,
        };
        let ir_dest = match lower_endpoint(&stmt.dest, name_to_id, ir) {
            Some(d) => d,
            None => continue,
        };

        // Type-compatibility check.
        let src_kind = endpoint_kind_of(&ir_source, ir);
        let dst_kind = ir.nodes[ir_dest.node]
            .endpoints
            .get(&ir_dest.endpoint)
            .map(|ei| ei.kind);

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

        match stmt.via {
            // -----------------------------------------------------------------
            // No via: single edge, not a feedback edge.
            // -----------------------------------------------------------------
            None => {
                insert_edge(
                    ir,
                    ir_source,
                    ir_dest,
                    stmt.policy,
                    stmt.span,
                    /*is_feedback=*/ false,
                );
            }

            // -----------------------------------------------------------------
            // Node via: expand `src -> [name] -> dst` into two edges:
            //   Edge 1: src   → via.input   (non-feedback)
            //   Edge 2: via.output → dst    (feedback)
            // -----------------------------------------------------------------
            Some(crate::ast::DelayVia::Node { name }) => {
                let via_id = match name_to_id.get(&name.to_string()) {
                    Some(&id) => id,
                    None => {
                        diags.push_error(syn::Error::new(
                            name.span(),
                            format!("unknown node `{}` in delay-route bracket", name),
                        ));
                        continue;
                    }
                };

                if !via_used_nodes.insert(via_id) {
                    diags.push_error(syn::Error::new(
                        name.span(),
                        format!(
                            "node `{}` is already wired by another `[{}]` reference",
                            name, name
                        ),
                    ));
                    continue;
                }

                // Edge 1: src → via.input
                let via_input = IrEndpoint {
                    node: via_id,
                    endpoint: Ident::new("input", name.span()),
                    index: None,
                    span: name.span(),
                    bare: false,
                };
                insert_edge(
                    ir,
                    ir_source,
                    via_input,
                    stmt.policy,
                    stmt.span,
                    /*is_feedback=*/ false,
                );

                // Edge 2: via.output → dst  (feedback — breaks the cycle)
                let via_output_expr = IrExpr {
                    kind: IrExprKind::Endpoint(IrEndpoint {
                        node: via_id,
                        endpoint: Ident::new("output", name.span()),
                        index: None,
                        span: name.span(),
                        bare: false,
                    }),
                    span: name.span(),
                };
                insert_edge(
                    ir,
                    via_output_expr,
                    ir_dest,
                    stmt.policy,
                    stmt.span,
                    /*is_feedback=*/ true,
                );
            }

            // -----------------------------------------------------------------
            // Samples via: synthesise an anonymous ::oscen::Delay node with N
            // samples and expand into two edges through it:
            //   Edge 1: src        → synth.input   (non-feedback)
            //   Edge 2: synth.output → dst          (feedback — breaks the cycle)
            // -----------------------------------------------------------------
            Some(crate::ast::DelayVia::Samples { value, span }) => {
                // Parse the literal sample count.
                let n: u32 = match value.base10_parse::<u32>() {
                    Ok(n) => n,
                    Err(err) => {
                        diags.push_error(err);
                        continue;
                    }
                };

                // Unique synthetic name for this inline delay.
                let synth_name = Ident::new(&format!("__inline_delay_{}", synth_counter), span);
                synth_counter += 1;

                // Build the constructor expression: ::oscen::Delay::new(N as f32, 0.0)
                let n_lit = proc_macro2::Literal::u32_unsuffixed(n);
                let ctor_expr: syn::Expr =
                    syn::parse_quote!(::oscen::Delay::new(#n_lit as f32, 0.0));
                let ty: syn::Path = syn::parse_quote!(::oscen::Delay);

                let synth_id = ir.nodes.insert_with_key(|id| IrNode {
                    id,
                    kind: IrNodeKind::Processor {
                        ty: Some(ty),
                        ctor_expr,
                    },
                    name: synth_name,
                    rate: crate::ast::NodeRate::Same,
                    latency_samples: 0,
                    span,
                    endpoints: synth_delay_endpoints(span),
                    incoming: Vec::new(),
                    outgoing: Vec::new(),
                });
                ir.processors.push(synth_id);

                // Edge 1: src → synth.input  (non-feedback)
                let synth_input = IrEndpoint {
                    node: synth_id,
                    endpoint: Ident::new("input", span),
                    index: None,
                    span,
                    bare: false,
                };
                insert_edge(
                    ir,
                    ir_source,
                    synth_input,
                    stmt.policy,
                    stmt.span,
                    /*is_feedback=*/ false,
                );

                // Edge 2: synth.output → dst  (feedback — breaks the cycle)
                let synth_output_expr = IrExpr {
                    kind: IrExprKind::Endpoint(IrEndpoint {
                        node: synth_id,
                        endpoint: Ident::new("output", span),
                        index: None,
                        span,
                        bare: false,
                    }),
                    span,
                };
                insert_edge(
                    ir,
                    synth_output_expr,
                    ir_dest,
                    stmt.policy,
                    stmt.span,
                    /*is_feedback=*/ true,
                );
            }
        }
    }
}

/// Insert one `IrEdge` into `ir`, updating adjacency lists and edge order.
fn insert_edge(
    ir: &mut IrGraph,
    source: IrExpr,
    dest: IrEndpoint,
    policy: ConnectionPolicy,
    span: proc_macro2::Span,
    is_feedback: bool,
) {
    // Compute primary source NodeId and extras from the IR source.
    let mut refs = collect_referenced_node_ids(&source);
    refs.dedup();
    let primary_src = match refs.first() {
        Some(&id) => id,
        None => return, // Pure-literal source with no node references; skip.
    };
    let extra_sources: Vec<NodeId> = refs.into_iter().skip(1).collect();

    let dest_node = dest.node;
    let extra_sources_clone = extra_sources.clone();
    let eid = ir.edges.insert_with_key(|id| IrEdge {
        id,
        source,
        dest,
        policy,
        kernel: EdgeKernel::None,
        fanout: FanoutShape::Scalar,
        span,
        extra_source_nodes: extra_sources_clone,
        is_feedback,
    });

    // Update adjacency and canonical edge order.
    ir.nodes[primary_src].outgoing.push(eid);
    for &extra in &extra_sources {
        ir.nodes[extra].outgoing.push(eid);
    }
    ir.nodes[dest_node].incoming.push(eid);
    ir.edge_order.push(eid);
}

/// Visitor that collects every `NodeId` referenced by an `IrExpr` in
/// left-to-right order, including duplicates (caller dedups if needed).
struct CollectEndpoints {
    ids: Vec<NodeId>,
}

impl CollectEndpoints {
    fn new() -> Self {
        Self { ids: Vec::new() }
    }
}

impl crate::ir::expr::visit::Visitor for CollectEndpoints {
    fn visit_endpoint(&mut self, ep: &crate::ir::expr::IrEndpoint) {
        self.ids.push(ep.node);
    }
}

/// Collect every `NodeId` referenced by an `IrExpr` source expression.
/// Used by `build_edges` to anchor edges whose source is a compound
/// expression — the first id is promoted to `IrEdge::source.node`, the
/// rest are stored in `extra_source_nodes`.
fn collect_referenced_node_ids(expr: &crate::ir::expr::IrExpr) -> Vec<NodeId> {
    use crate::ir::expr::visit::Visitor;
    let mut v = CollectEndpoints::new();
    v.visit_expr(expr);
    v.ids
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
            let src_node_id = match primary_node(&edge.source) {
                Some(id) => id,
                None => continue, // Pure-literal source; no rate to check.
            };
            (src_node_id, edge.dest.node, edge.policy, edge.span)
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
        let (src_node_id, dst_node_id, src_kind, dst_kind) = {
            let edge = &ir.edges[eid];
            let src_node_id = match primary_node(&edge.source) {
                Some(id) => id,
                None => continue,
            };
            let dst_node_id = edge.dest.node;
            let src_kind = endpoint_kind_of(&edge.source, ir);
            let dst_kind = ir.nodes[dst_node_id]
                .endpoints
                .get(&edge.dest.endpoint)
                .map(|e| e.kind);
            (src_node_id, dst_node_id, src_kind, dst_kind)
        };

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

/// Infer the `EndpointKind` of an `IrExpr` using the IR's resolved node
/// registry. This is the single source of truth for endpoint-kind
/// inference; codegen's `infer_kind` is a thin delegator.
///
/// Returns `None` for `MethodCall` and `Call` (no type inference for
/// arbitrary Rust function/method return types).
pub(crate) fn endpoint_kind_of(expr: &IrExpr, ir: &IrGraph) -> Option<EndpointKind> {
    match &expr.kind {
        IrExprKind::Endpoint(ep) => ir.nodes[ep.node]
            .endpoints
            .get(&ep.endpoint)
            .map(|ei| ei.kind),
        IrExprKind::Binary { left, right, .. } => {
            let l = endpoint_kind_of(left, ir)?;
            let r = endpoint_kind_of(right, ir)?;
            match (l, r) {
                (EndpointKind::Stream, EndpointKind::Stream) => Some(EndpointKind::Stream),
                (EndpointKind::Stream, EndpointKind::Value) => Some(EndpointKind::Stream),
                (EndpointKind::Value, EndpointKind::Stream) => Some(EndpointKind::Stream),
                (EndpointKind::Value, EndpointKind::Value) => Some(EndpointKind::Value),
                (EndpointKind::Event, _) | (_, EndpointKind::Event) => None,
            }
        }
        IrExprKind::Literal(_) => Some(EndpointKind::Value),
        IrExprKind::MethodCall { .. } | IrExprKind::Call { .. } => None,
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

// ---------------------------------------------------------------------------
// Step 6: Topological sort
// ---------------------------------------------------------------------------

/// Step 6: Sort `ir.processors` into topological (dependency) order using
/// Kahn's algorithm.
///
/// Edges marked as feedback (`edge.is_feedback`, set on the outgoing leg of
/// an inline-delay `-> [N] ->` / `-> [name] ->` expansion) are excluded
/// from both in-degree counting and outgoing propagation so a cycle closed
/// by such an edge does not appear as a real cycle. Emits a "non-feedback
/// cycle" error into `diags` if the graph is cyclic after removing feedback
/// edges.
fn topo_sort(ir: &mut IrGraph, diags: &mut Diagnostics) {
    use std::collections::{HashMap, VecDeque};

    let processor_set: std::collections::HashSet<NodeId> = ir.processors.iter().copied().collect();

    // Compute in-degree for each processor from non-feedback edges whose
    // source is also a processor. Feedback edges are skipped entirely.
    let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
    for &nid in &ir.processors {
        in_degree.insert(nid, 0);
    }
    for &nid in &ir.processors {
        for &eid in &ir.nodes[nid].incoming {
            let edge = &ir.edges[eid];
            if edge.is_feedback {
                continue;
            }
            let mut count_src = |src: NodeId| {
                if processor_set.contains(&src) {
                    *in_degree.get_mut(&nid).unwrap() += 1;
                }
            };
            if let Some(primary) = primary_node(&edge.source) {
                count_src(primary);
            }
            for &extra in &edge.extra_source_nodes {
                count_src(extra);
            }
        }
    }

    let mut queue: VecDeque<NodeId> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut sorted: Vec<NodeId> = Vec::with_capacity(ir.processors.len());

    while let Some(nid) = queue.pop_front() {
        sorted.push(nid);
        // Outgoing feedback edges don't impose ordering, mirroring the
        // in-degree pass above. (Edges OUT of this node that are feedback
        // edges contribute zero to anybody's in-degree, so they're never
        // decremented.)
        let outgoing: Vec<EdgeId> = ir.nodes[nid].outgoing.clone();
        for eid in outgoing {
            let edge = &ir.edges[eid];
            if edge.is_feedback {
                continue;
            }
            let dst = edge.dest.node;
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
            "graph contains a non-feedback cycle (use `-> [N] ->` to insert a delay buffer, or `-> [delay_node] ->` to route through a declared Delay node)",
        ));
        return;
    }
    ir.processors = sorted;
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

        let src_kind = endpoint_kind_of(&edge.source, ir);
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
#[allow(clippy::only_used_in_recursion)] // `ir` reserved for future endpoint validation
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
                    bare: true,
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
                    bare: false,
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
                bare,
            }) = inner_expr.kind
            {
                Some(IrExpr {
                    kind: IrExprKind::Endpoint(IrEndpoint {
                        node,
                        endpoint,
                        index: Some(*idx),
                        span,
                        bare,
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
            let ir_args: Option<Vec<_>> =
                args.iter().map(|a| lower_expr(a, name_to_id, ir)).collect();
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
                bare: true,
            })
        }
        ConnectionExpr::Field(obj, field) => {
            let (node, index, _anchor) = resolve_field_base(obj, name_to_id)?;
            Some(IrEndpoint {
                node,
                endpoint: field.clone(),
                index,
                span: field.span(),
                bare: false,
            })
        }
        _ => None,
    }
}
