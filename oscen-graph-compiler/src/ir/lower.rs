//! AST → IR lowering.
//!
//! `lower(graph_def, diags)` walks the eight analysis steps in order
//! and populates an `IrGraph`. Steps are private free functions —
//! they're one-shot lowering helpers, not part of the IR's public
//! mutation API. Accumulates diagnostics across all steps and returns
//! `None` if any errors landed.

use crate::ast::{
    ConnectionExpr, ConnectionPolicy, ConnectionStmt, EndpointKind, GraphDef, GraphItem, NodeRate,
};
use crate::codegen::helpers::ident_base;
use crate::diagnostics::Diagnostics;
use crate::ir::expr::{primary_node, IrEndpoint, IrExpr, IrExprKind};
use crate::ir::graph::{
    classify_fanout, AssetBinding, EdgeId, EdgeKernel, EndpointInfo, EventRescale, FanoutShape,
    IrEdge, IrGraph, IrNode, IrNodeKind, NodeId,
};
use proc_macro2::Span;
use std::collections::{HashMap, HashSet};
use syn::Ident;

pub fn lower(mut graph_def: GraphDef, diags: &mut Diagnostics) -> Option<IrGraph> {
    expand_hoists(&mut graph_def, diags);
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
    validate_typed_value_endpoints(&ir, diags);

    #[cfg(debug_assertions)]
    crate::ir::validate::validate(&ir);

    if diags.is_empty() {
        Some(ir)
    } else {
        None
    }
}

/// Step 0: Expand hoisted endpoint declarations.
///
/// `input voices.cutoff;` declares a graph input *and* re-exports the child
/// endpoint, so each hoist synthesizes the corresponding connection
/// statement (`cutoff -> voices.cutoff`) as if the user had written it in a
/// `connections {}` block. Everything downstream (type inference,
/// broadcast-to-array fan-out, edge building, ramp plumbing) treats the
/// synthesized statement identically to a hand-written one.
///
/// Also validates that the hoisted node name refers to a declared node —
/// with a dedicated message, because at connection-lowering time the
/// generic "cannot resolve" error would point at a connection the user
/// never wrote.
/// Expand endpoint-list hoists (`input node.{a, b} pat_*;`) into one
/// single-endpoint hoist InputDecl per endpoint, in place (so declaration
/// order — and thus param-registry order — is preserved).
///
/// Runs as a pre-pass in `compile_parsed` and again as part of hoist
/// expansion here (idempotent — a no-op when the pre-pass already ran —
/// so direct `lower` callers keep working).
///
/// When the hoisted node's endpoint manifest happens to be resolved
/// (`manifests` is populated for nodes that are wildcard-hoisted
/// elsewhere in the graph), the child endpoint's declared `ty` is threaded
/// into the synthesized decl — same rule as wildcard expansion — so a
/// typed value endpoint (or a `Frame<N>` stream) hoists with its type
/// instead of collapsing to mono `f32`. List hoists alone don't trigger
/// manifest resolution, so without one the decl stays untyped (`f32`) and
/// a typed child endpoint surfaces as rustc's `ConnectEndpoints<f32, T>`
/// error — hoist such endpoints explicitly (`input node.ep: value: T;`).
pub(crate) fn expand_list_hoists(
    graph_def: &mut GraphDef,
    manifests: &HashMap<String, crate::manifest::NodeManifest>,
    diags: &mut Diagnostics,
) {
    use crate::ast::{HoistEndpoints, HoistSource, InputDecl};

    let items = std::mem::take(&mut graph_def.items);
    let mut expanded: Vec<GraphItem> = Vec::with_capacity(items.len());
    for item in items {
        let GraphItem::Input(input) = item else {
            expanded.push(item);
            continue;
        };
        let Some(HoistSource {
            node,
            endpoints: HoistEndpoints::List { endpoints, rename },
        }) = input.hoist.clone()
        else {
            expanded.push(GraphItem::Input(input));
            continue;
        };
        let manifest = manifests.get(&node.to_string());
        for endpoint in endpoints {
            let name = match &rename {
                Some(pat) => match pat.try_apply(&endpoint) {
                    Ok(name) => name,
                    Err(e) => {
                        diags.push_error(e);
                        continue;
                    }
                },
                None => endpoint.clone(),
            };
            // Inherit the child endpoint's declared type from the manifest
            // where one is resolved; `None` stays mono `f32`, as before.
            let ty = manifest
                .and_then(|m| m.inputs.iter().find(|ep| ep.name == endpoint))
                .and_then(|ep| ep.ty.clone());
            expanded.push(GraphItem::Input(InputDecl {
                kind: input.kind,
                name,
                ty,
                default: None,
                spec: None,
                hoist: Some(HoistSource {
                    node: node.clone(),
                    endpoints: HoistEndpoints::Single(endpoint),
                }),
            }));
        }
    }
    graph_def.items = expanded;
}

fn expand_hoists(graph_def: &mut GraphDef, diags: &mut Diagnostics) {
    // Names of declared nodes/arrays, for validation.
    let node_names = graph_def.node_decl_names();

    // Pass 1: expand endpoint-list hoists into single-endpoint hoists.
    // No manifests here: `compile_parsed` already ran the manifest-aware
    // pre-pass, so this call is a no-op for it; direct `lower` callers
    // get the untyped (`f32`) expansion, as before.
    expand_list_hoists(graph_def, &HashMap::new(), diags);

    // (node, endpoint) pairs that already have a user-written driver. A
    // hoist synthesizes its own `input -> node.endpoint` write, so hoisting
    // an endpoint that is also a connection dest would give it two drivers
    // — and since synthesized statements run last, the user's edge would
    // silently lose. Indexed roots unwrap so `voices[0].freq` conflicts
    // with a broadcast hoist of `voices.freq`. Keys are r#-stripped: Rust
    // treats `foo` and `r#foo` as the same field, so the raw spelling must
    // not slip past the check.
    let mut driven: HashSet<(String, String)> = HashSet::new();
    let record_dest = |driven: &mut HashSet<(String, String)>, dest: &ConnectionExpr| {
        if let ConnectionExpr::Field(root, endpoint) = dest {
            let mut inner: &ConnectionExpr = root;
            while let ConnectionExpr::ArrayIndex(next, _) = inner {
                inner = next;
            }
            if let ConnectionExpr::Ident(node) = inner {
                driven.insert((ident_base(node), ident_base(endpoint)));
            }
        }
    };
    for item in &graph_def.items {
        match item {
            GraphItem::Connection(stmt) => record_dest(&mut driven, &stmt.dest),
            GraphItem::ConnectionBlock(block) => {
                for stmt in &block.0 {
                    record_dest(&mut driven, &stmt.dest);
                }
            }
            _ => {}
        }
    }

    // Pass 2: validate node names and synthesize the connections.
    let mut synthesized: Vec<GraphItem> = Vec::new();
    for item in &graph_def.items {
        let GraphItem::Input(input) = item else {
            continue;
        };
        let Some(hoist) = &input.hoist else {
            continue;
        };
        let Some(endpoint) = hoist.single_endpoint() else {
            // List hoists were expanded in pass 1; wildcard hoists are
            // expanded (or rejected) before lowering by
            // `manifest::expand_wildcards`. A leftover here means a caller
            // bypassed that pass — report instead of panicking.
            diags.push_error(syn::Error::new(
                hoist.node.span(),
                format!(
                    "internal error: unexpanded hoist on node `{}` reached lowering",
                    hoist.node
                ),
            ));
            continue;
        };

        if !node_names.contains(&hoist.node.to_string()) {
            diags.push_error(syn::Error::new(
                hoist.node.span(),
                format!(
                    "hoisted input `{}.{}` references unknown node `{}` \
                     (hoists re-export a declared node's endpoint: \
                     `input <node>.<endpoint>;`)",
                    hoist.node, endpoint, hoist.node
                ),
            ));
            continue;
        }

        if !driven.insert((ident_base(&hoist.node), ident_base(endpoint))) {
            diags.push_error(syn::Error::new(
                input.name.span(),
                format!(
                    "hoisted input `{}` re-exports `{}.{}`, which already has a \
                     driver (an explicit connection or another hoist); the hoist \
                     synthesizes `{} -> {}.{}`, so the endpoint would have two \
                     drivers — remove one",
                    input.name, hoist.node, endpoint, input.name, hoist.node, endpoint
                ),
            ));
            continue;
        }

        let span = input.name.span();
        synthesized.push(GraphItem::Connection(ConnectionStmt {
            source: ConnectionExpr::Ident(input.name.clone()),
            dest: ConnectionExpr::Field(
                Box::new(ConnectionExpr::Ident(hoist.node.clone())),
                endpoint.clone(),
            ),
            policy: ConnectionPolicy::Default,
            span,
            via: None,
        }));
    }
    graph_def.items.extend(synthesized);
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
    // Externals claim names too (`declared_names()` agrees): without this,
    // an input or hoist sharing an external's name slips past the duplicate
    // check and `build_edges` silently reclassifies its connection as an
    // asset binding.
    let mut external_names: HashSet<String> = HashSet::new();
    for item in &graph_def.items {
        match item {
            GraphItem::Input(input) => {
                let id = ir.nodes.insert_with_key(|id| IrNode {
                    id,
                    kind: IrNodeKind::Input {
                        spec: input.spec.clone(),
                        default: input.default.clone(),
                        hoist: input.hoist.clone(),
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
                if name_to_id.insert(input.name.to_string(), id).is_some()
                    || external_names.contains(&input.name.to_string())
                {
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
                if name_to_id.insert(output.name.to_string(), id).is_some()
                    || external_names.contains(&output.name.to_string())
                {
                    diags.push_error(syn::Error::new(
                        output.name.span(),
                        format!("duplicate declaration of `{}`", output.name),
                    ));
                }
            }
            GraphItem::Node(node) => {
                collect_node_decl(node, ir, name_to_id, &external_names, diags);
            }
            GraphItem::NodeBlock(block) => {
                for n in &block.0 {
                    collect_node_decl(n, ir, name_to_id, &external_names, diags);
                }
            }
            // An `external` is not a processing node: record it as a
            // graph-boundary asset handle. Its `-> node.asset` binding is
            // resolved in `build_edges`.
            GraphItem::External(ext) => {
                let name = ext.name.to_string();
                if name_to_id.contains_key(&name) || !external_names.insert(name) {
                    diags.push_error(syn::Error::new(
                        ext.name.span(),
                        format!("duplicate declaration of `{}`", ext.name),
                    ));
                }
                ir.externals.push(ext.clone());
            }
            // Connections + nih_params + name don't create IrNodes here;
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
    external_names: &HashSet<String>,
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
    if name_to_id.insert(decl.name.to_string(), id).is_some()
        || external_names.contains(&decl.name.to_string())
    {
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
        EndpointInfo::new(EndpointKind::Stream),
    );
    m.insert(
        Ident::new("output", span),
        EndpointInfo::new(EndpointKind::Stream),
    );
    m.insert(
        Ident::new("delay_samples", span),
        EndpointInfo::new(EndpointKind::Value),
    );
    m.insert(
        Ident::new("feedback", span),
        EndpointInfo::new(EndpointKind::Value),
    );
    m
}

fn input_endpoints(input: &crate::ast::InputDecl) -> HashMap<Ident, EndpointInfo> {
    let mut m = HashMap::new();
    // The "name" of the implicit endpoint on an input/output decl is
    // the decl's own identifier — `s -> osc.frequency` references the
    // `s` endpoint on the `s` input node.
    m.insert(
        input.name.clone(),
        EndpointInfo {
            kind: input.kind,
            ty: input.ty.clone(),
        },
    );
    m
}

fn output_endpoints(output: &crate::ast::OutputDecl) -> HashMap<Ident, EndpointInfo> {
    let mut m = HashMap::new();
    m.insert(
        output.name.clone(),
        EndpointInfo {
            kind: output.kind,
            ty: output.ty.clone(),
        },
    );
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

    // Seed Stream kind from explicit stream-resampling policies.
    //
    // `[linear]`/`[sinc]`/`[sinc_iir]` are stream-only interpolation/decimation
    // filters — they have no value-edge interpretation (unlike `[latch]`, which
    // `refine_kernels` also uses as the value cross-rate default). So both ends
    // of such a connection must be `Stream`. In a pure node-to-node graph with
    // no graph-level stream I/O, this is the only signal that classifies these
    // processor-node endpoints as `Stream`; without it nothing seeds the
    // propagation below and the `CrossRateKernel` projection falls back to the
    // concrete `f32` kernel. Only fills vacant entries, so it never overrides a
    // kind already known from a graph endpoint or the derive.
    for stmt in &stmts {
        if !matches!(
            stmt.policy,
            ConnectionPolicy::Linear | ConnectionPolicy::Sinc | ConnectionPolicy::SincIir
        ) {
            continue;
        }
        for expr in [&stmt.source, &stmt.dest] {
            if let Some((node_id, ep)) = resolve_node_endpoint(expr, name_to_id) {
                ir.nodes[node_id]
                    .endpoints
                    .entry(ep)
                    .or_insert(EndpointInfo::new(EndpointKind::Stream));
            }
        }
    }

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
                            e.insert(EndpointInfo::new(src_kind));
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
                            e.insert(EndpointInfo::new(dst_kind));
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

    // External (asset) names declared in this graph. An edge whose *source*
    // is one of these is an asset binding, not a signal edge.
    let external_names: std::collections::HashSet<String> =
        ir.externals.iter().map(|e| e.name.to_string()).collect();

    // Pre-pass: resolve `external -> node.asset` bindings. These are not signal
    // edges — they carry no kernel, impose no processing order, and create no
    // `IrEdge`. Recording them up front (and marking the destination endpoint
    // `Asset`) lets the main pass reject any *other* edge into an asset input.
    let mut binding_stmts: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (i, stmt) in stmts.iter().enumerate() {
        let src_ident = match &stmt.source {
            ConnectionExpr::Ident(id) if external_names.contains(&id.to_string()) => id,
            _ => continue,
        };
        // This statement is an asset binding regardless of how it resolves.
        binding_stmts.insert(i);

        let (dst_node, dst_ep) = match resolve_node_endpoint(&stmt.dest, name_to_id) {
            Some(d) => d,
            None => {
                diags.push_error(syn::Error::new(
                    stmt.span,
                    format!(
                        "external `{}` can only be bound to a node's asset input \
                         (`{} -> node.asset`)",
                        src_ident, src_ident
                    ),
                ));
                continue;
            }
        };
        // The target must be a single processor node with a known type so the
        // `AssetEndpoint` trait can be projected during codegen.
        if !matches!(
            ir.nodes[dst_node].kind,
            IrNodeKind::Processor { ty: Some(_), .. }
        ) {
            diags.push_error(syn::Error::new(
                stmt.span,
                format!(
                    "external `{}` must be bound to a single node with a known type",
                    src_ident
                ),
            ));
            continue;
        }

        // Mark the endpoint `Asset` so a stray signal edge into it is caught.
        ir.nodes[dst_node]
            .endpoints
            .insert(dst_ep.clone(), EndpointInfo::new(EndpointKind::Asset));

        ir.asset_bindings.push(AssetBinding {
            external_name: src_ident.clone(),
            node: dst_node,
            endpoint: dst_ep,
        });
    }

    let mut synth_counter: u32 = 0;
    let mut via_used_nodes: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for (stmt_index, stmt) in stmts.into_iter().enumerate() {
        // Asset bindings were resolved in the pre-pass; they create no edge.
        if binding_stmts.contains(&stmt_index) {
            continue;
        }
        // Lower to IR forms. A failed lowering means a name in the expression
        // resolved to nothing — report it; a silently dropped edge compiles to
        // a graph that runs but produces silence.
        let ir_source = match lower_expr(&stmt.source, name_to_id, ir) {
            Some(e) => e,
            None => {
                diags.push_error(syn::Error::new(
                    stmt.span,
                    "cannot resolve the source of this connection \
                     (unknown node or endpoint name?)",
                ));
                continue;
            }
        };
        let ir_dest = match lower_endpoint(&stmt.dest, name_to_id, ir) {
            Some(d) => d,
            None => {
                diags.push_error(syn::Error::new(
                    stmt.span,
                    "cannot resolve the destination of this connection \
                     (unknown node or endpoint name?)",
                ));
                continue;
            }
        };

        // Type-compatibility check.
        let src_kind = endpoint_kind_of(&ir_source, ir);
        let dst_kind = ir.nodes[ir_dest.node]
            .endpoints
            .get(&ir_dest.endpoint)
            .map(|ei| ei.kind);

        // An asset input can only be fed by an `external` (handled in the
        // pre-pass). Any other source into it is out of scope in v1.
        if matches!(dst_kind, Some(EndpointKind::Asset)) {
            diags.push_error(syn::Error::new(
                stmt.span,
                format!(
                    "asset input `{}.{}` can only be bound from an `external` in v1",
                    ir.nodes[ir_dest.node].name, ir_dest.endpoint
                ),
            ));
            continue;
        }

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
                    diags,
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
                    diags,
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
                    diags,
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
                    diags,
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
                    diags,
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
    diags: &mut Diagnostics,
) {
    // Compute primary source NodeId and extras from the IR source.
    let mut refs = collect_referenced_node_ids(&source);
    refs.dedup();
    let primary_src = match refs.first() {
        Some(&id) => id,
        None => {
            // A source with no node references (e.g. `0.5 -> g.gain;`) has no
            // edge to anchor on; silently dropping it would compile to a graph
            // that never delivers the value.
            diags.push_error(syn::Error::new(
                span,
                "constant connection sources are not supported; \
                 set a default on the destination input instead",
            ));
            return;
        }
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
/// rest are stored in `extra_source_nodes`. Also used by codegen's
/// post-inner taint analysis to consider every referenced source node.
pub(crate) fn collect_referenced_node_ids(expr: &crate::ir::expr::IrExpr) -> Vec<NodeId> {
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
        let (src_node_id, dst_node_id, policy, span, src_index, dst_index) = {
            let edge = &ir.edges[eid];
            let src_node_id = match primary_node(&edge.source) {
                Some(id) => id,
                None => continue, // Pure-literal source; no rate to check.
            };
            let src_index = match &edge.source.kind {
                IrExprKind::Endpoint(ep) => ep.index,
                _ => None,
            };
            (
                src_node_id,
                edge.dest.node,
                edge.policy,
                edge.span,
                src_index,
                edge.dest.index,
            )
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

        // Compute fanout shape from source/dest node array sizes. An indexed
        // endpoint (`voices[0].output`, `voices[2].frequency`) addresses one
        // element, so it is scalar regardless of the node's array size.
        let src_array_size =
            array_size_of(&ir.nodes[src_node_id].kind).filter(|_| src_index.is_none());
        let dst_array_size =
            array_size_of(&ir.nodes[dst_node_id].kind).filter(|_| dst_index.is_none());
        let fanout = classify_fanout(src_array_size, dst_array_size);

        ir.edges[eid].kernel = kernel;
        ir.edges[eid].fanout = fanout;
    }
}

/// Per-node rate validation. Catches `Down(n)` rate annotations
/// (currently unsupported) even on nodes with no edges. Mirrors the
/// per-node check in `rate_analysis::analyze` so that unconnected
/// undersampled nodes also produce a diagnostic. Also rejects graphs
/// mixing different `Up(n)` oversampling factors.
fn validate_node_rates(ir: &IrGraph, diags: &mut Diagnostics) {
    // Mixed `* N` factors are rejected for now: codegen runs the inner loop
    // to the max factor while sizing and indexing each Up/Down edge buffer by
    // its own edge's factor, so mixed factors would index out of bounds at
    // runtime. Future upgrade path (clock division): gate each node's inner
    // work on `__inner % (max / factor) == 0` and index its buffers by
    // `__inner / (max / factor)`; factors are powers of two, so LCM == max.
    let mut first_up: Option<u32> = None;
    for &id in &ir.processors {
        let node = &ir.nodes[id];
        match node.rate {
            NodeRate::Down(n) if n > 1 => {
                diags.push_error(syn::Error::new(
                    node.span,
                    "node undersampling (`/ N`) is not yet supported in v1; only oversampling (`* N`) is implemented",
                ));
            }
            NodeRate::Up(n) => match first_up {
                None => first_up = Some(n),
                Some(m) if m != n => {
                    diags.push_error(syn::Error::new(
                        node.span,
                        format!(
                            "all oversampled nodes in a graph must use the same rate factor \
                             (found `* {}` and `* {}`); use the highest factor for every \
                             oversampled node",
                            m, n
                        ),
                    ));
                }
                Some(_) => {}
            },
            _ => {}
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
                // Asset endpoints never participate in binary signal expressions.
                (EndpointKind::Asset, _) | (_, EndpointKind::Asset) => None,
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

    // Seed the queue in declaration order (not HashMap iteration order) so
    // the topological sort — and thus generated code — is deterministic for
    // independent nodes. Ties broken by source order.
    let mut queue: VecDeque<NodeId> = ir
        .processors
        .iter()
        .copied()
        .filter(|id| in_degree[id] == 0)
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
        // Reconstruct a concrete cycle among the unsorted (cyclic-component)
        // nodes so the error names the offending path instead of leaving the
        // user to bisect their connections by hand.
        let sorted_set: std::collections::HashSet<NodeId> = sorted.iter().copied().collect();
        let remaining: Vec<NodeId> = ir
            .processors
            .iter()
            .copied()
            .filter(|id| !sorted_set.contains(id))
            .collect();
        let (cycle_desc, cycle_span) = describe_cycle(ir, &remaining);
        diags.push_error(syn::Error::new(
            cycle_span,
            format!(
                "graph contains a non-feedback cycle: {cycle_desc}. \
                 If this loop is intentional feedback, break it with an inline \
                 delay on one edge (`src -> [N] -> dst`, N >= 1 samples) or \
                 route it through a declared Delay node (`src -> [delay_node] -> dst`). \
                 If it is unintentional, one of these connections points the \
                 wrong way."
            ),
        ));
        return;
    }
    ir.processors = sorted;
}

/// Walk non-feedback edges among `remaining` (the nodes Kahn's algorithm
/// could not order) until a node repeats, then render the closed walk as
/// `a -> b -> ... -> a`. Returns the description plus the span of the first
/// edge on the cycle for diagnostics.
fn describe_cycle(ir: &IrGraph, remaining: &[NodeId]) -> (String, proc_macro2::Span) {
    let remaining_set: std::collections::HashSet<NodeId> = remaining.iter().copied().collect();
    if remaining.is_empty() {
        return (
            "(unable to reconstruct the cycle)".to_string(),
            proc_macro2::Span::call_site(),
        );
    }

    // `remaining` holds every node Kahn's algorithm could not order, which
    // includes acyclic nodes strictly *downstream* of a cycle (their
    // in-degree never reaches 0 either). A depth-first search with the usual
    // three-color marking finds a back edge — and thus a cycle — from any
    // start that can reach one, regardless of edge declaration order (a
    // greedy single-path walk can be steered into a dead end by an
    // unluckily-ordered branch off the cycle). O(V + E).
    let mut visited: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
    for &start in remaining {
        if visited.contains(&start) {
            continue;
        }
        // Explicit DFS stack: (node, index into its outgoing edges).
        // `path`/`on_path` hold the gray chain; `path_spans[i]` is the span
        // of the edge `path[i] -> path[i+1]`.
        let mut stack: Vec<(NodeId, usize)> = vec![(start, 0)];
        let mut path: Vec<NodeId> = vec![start];
        let mut on_path: std::collections::HashSet<NodeId> = std::iter::once(start).collect();
        let mut path_spans: Vec<proc_macro2::Span> = Vec::new();
        while let Some(frame) = stack.last_mut() {
            let current = frame.0;
            let outgoing = &ir.nodes[current].outgoing;
            let mut hop = None;
            while frame.1 < outgoing.len() {
                let eid = outgoing[frame.1];
                frame.1 += 1;
                let edge = &ir.edges[eid];
                if edge.is_feedback {
                    continue;
                }
                let dst = edge.dest.node;
                if remaining_set.contains(&dst) && !visited.contains(&dst) {
                    hop = Some((dst, edge.span));
                    break;
                }
            }
            let Some((next, span)) = hop else {
                // All outgoing edges exhausted: blacken and pop.
                visited.insert(current);
                on_path.remove(&current);
                path.pop();
                path_spans.pop();
                stack.pop();
                continue;
            };
            if on_path.contains(&next) {
                // Back edge: path[pos..] ++ next closes the loop.
                let pos = path.iter().position(|&n| n == next).unwrap();
                let names: Vec<String> = path[pos..]
                    .iter()
                    .chain(std::iter::once(&next))
                    .map(|&id| format!("`{}`", ir.nodes[id].name))
                    .collect();
                // Span of the first edge on the cycle: the edge leaving
                // `path[pos]`, or the closing back edge for a self-loop /
                // top-of-path cycle.
                let span = path_spans.get(pos).copied().unwrap_or(span);
                return (names.join(" -> "), span);
            }
            on_path.insert(next);
            path.push(next);
            path_spans.push(span);
            stack.push((next, 0));
        }
    }

    // Fallback: list the nodes involved. Unreachable when a true cycle
    // exists (some start must close a loop), but kept as a safety net.
    let names: Vec<String> = remaining
        .iter()
        .map(|&id| format!("`{}`", ir.nodes[id].name))
        .collect();
    (
        format!("involving nodes {}", names.join(", ")),
        proc_macro2::Span::call_site(),
    )
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

// ---------------------------------------------------------------------------
// Step 9: Typed value endpoint validation
// ---------------------------------------------------------------------------

/// Step 9: Validate the constraints on TYPED value endpoints (graph value
/// inputs/outputs declared with a non-`f32` type; see
/// [`EndpointInfo::typed_value_ty`]):
///
/// - **(a) No param specs.** Typed inputs are excluded from the param
///   registry, so ramps, ranges, and display metadata have nothing to attach
///   to (and typed payloads cannot interpolate).
/// - **(b) No fan-in.** Values don't sum (Cmajor precedent): a typed dest
///   slot takes exactly one source, and an array fan-in (`voices.mode ->
///   out_mode` summing all elements) is likewise rejected. Plain f32 VALUE
///   dests are held to the same rule — only streams sum — so two sources
///   into one value endpoint is an error, not last-write-wins.
/// - **(c) Latch-only across rate boundaries.** A cross-rate typed value
///   edge is emitted as a copy at the outer-block boundary; interpolating
///   policies (`[linear]`, `[sinc]`, ...) are meaningless for opaque
///   payloads.
///
/// Only graph-boundary endpoints carry declared types in the IR; typed
/// *node* fields are invisible here and enforced by rustc through the
/// `ConnectEndpoints` bounds. All violations are reported, none bail early.
fn validate_typed_value_endpoints(ir: &IrGraph, diags: &mut Diagnostics) {
    // (a) Param specs on typed value inputs.
    for &id in &ir.inputs {
        let node = &ir.nodes[id];
        let Some(ty) = ir.typed_value_endpoint_ty(id, &node.name) else {
            continue;
        };
        let has_spec = matches!(&node.kind, IrNodeKind::Input { spec: Some(_), .. });
        if has_spec {
            let ty_str = quote::quote!(#ty).to_string().replace(' ', "");
            diags.push_error(syn::Error::new(
                node.span,
                format!(
                    "typed value input `{}` cannot carry a param spec: `{}` is not an \
                     f32 parameter, so ranges, ramps, and display metadata do not \
                     apply; drop the `[...]`/`{{...}}` spec or declare the input as f32",
                    node.name, ty_str,
                ),
            ));
        }
    }

    // (b) Fan-in into a typed value dest, and (c) non-latch cross-rate
    // policies on typed value edges. Group edges by dest slot; a bucket is
    // "typed" if any of its edges touches a typed graph endpoint. The dest's
    // endpoint kind rides along so plain f32 VALUE fan-in is rejected too
    // (values don't sum; only streams do).
    type Bucket = (Vec<EdgeId>, bool, Option<EndpointKind>);
    let mut buckets: HashMap<(NodeId, String, Option<usize>), Bucket> = HashMap::new();
    for &eid in &ir.edge_order {
        let edge = &ir.edges[eid];
        // Every edge participates in dest buckets so that a typed source
        // fanning in alongside an untyped one is still caught; the checks
        // below only fire on buckets that contain at least one typed edge
        // or a value-kind dest.
        let dest = &edge.dest;
        let bucket = buckets
            .entry((dest.node, dest.endpoint.to_string(), dest.index))
            .or_default();
        bucket.0.push(eid);
        let typed = ir.edge_is_typed_value(edge);
        bucket.1 |= typed;
        let dest_kind = ir.nodes[dest.node]
            .endpoints
            .get(&dest.endpoint)
            .map(|ei| ei.kind);
        if bucket.2.is_none() {
            bucket.2 = dest_kind;
        }

        // Array fan-in shape sums element values — impossible for typed
        // payloads and equally disallowed for plain f32 value dests.
        if let FanoutShape::FanIn { .. } = edge.fanout {
            if typed {
                diags.push_error(syn::Error::new(
                    edge.span,
                    "typed values cannot fan in from a node array: values don't sum; \
                     index one element (`voices[0].mode`) or restructure",
                ));
            } else if matches!(dest_kind, Some(EndpointKind::Value)) {
                diags.push_error(syn::Error::new(
                    edge.span,
                    "values cannot fan in from a node array (streams sum; values \
                     don't); index one element (`voices[0].out`) or restructure",
                ));
            }
        }

        if !typed {
            continue;
        }

        // (c) latch-only across rate boundaries.
        match edge.kernel {
            EdgeKernel::Up { kind, .. } | EdgeKernel::Down { kind, .. }
                if !matches!(kind, ConnectionPolicy::Latch) =>
            {
                diags.push_error(syn::Error::new(
                    edge.span,
                    "typed value connections are latch-only across rate boundaries \
                     (the value is copied once per outer block); remove the resampling \
                     policy annotation",
                ));
            }
            _ => {}
        }
    }

    // A broadcast dest (`voices.gain`) drives every element, so it overlaps
    // any indexed dest (`voices[0].gain`) on the same endpoint: that element
    // gets two drivers and connection order decides which wins — the same
    // silent clobber the per-slot rule below rejects.
    let mut broadcasts: HashMap<(NodeId, &str), (bool, Option<EndpointKind>)> = HashMap::new();
    for ((node, endpoint, index), (_, has_typed, kind)) in &buckets {
        if index.is_none() {
            broadcasts.insert((*node, endpoint.as_str()), (*has_typed, *kind));
        }
    }
    for ((node, endpoint, index), (edges, has_typed, kind)) in &buckets {
        let Some(i) = index else { continue };
        let Some((b_typed, b_kind)) = broadcasts.get(&(*node, endpoint.as_str())) else {
            continue;
        };
        let is_value = matches!(kind.or(*b_kind), Some(EndpointKind::Value));
        if !(*has_typed || *b_typed || is_value) {
            continue;
        }
        let dest_name = &ir.nodes[*node].name;
        for &eid in edges {
            diags.push_error(syn::Error::new(
                ir.edges[eid].span,
                format!(
                    "value endpoint `{dest_name}[{i}].{endpoint}` is driven both directly \
                     and by a broadcast connection to `{dest_name}.{endpoint}` (values \
                     cannot fan in); drop one of the two drivers",
                ),
            ));
        }
    }
    for ((dest_node, dest_endpoint, _), (edges, has_typed, kind)) in buckets {
        if edges.len() < 2 {
            continue;
        }
        let is_value = matches!(kind, Some(EndpointKind::Value));
        if !has_typed && !is_value {
            continue;
        }
        let dest_name = &ir.nodes[dest_node].name;
        let dest_desc = if dest_name.to_string() == dest_endpoint {
            dest_name.to_string()
        } else {
            format!("{dest_name}.{dest_endpoint}")
        };
        for &eid in &edges[1..] {
            let msg = if has_typed {
                format!(
                    "typed value endpoint `{dest_desc}` has {} sources, but typed \
                     values cannot fan in (values don't sum); keep a single source",
                    edges.len(),
                )
            } else {
                format!(
                    "value endpoint `{dest_desc}` has {} sources, but values cannot \
                     fan in (streams sum; values don't); combine them explicitly \
                     (`a + b -> {dest_desc}`) or keep a single source",
                    edges.len(),
                )
            };
            diags.push_error(syn::Error::new(ir.edges[eid].span, msg));
        }
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
        EndpointKind::Asset => "asset",
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
            let span = func
                .segments
                .last()
                .map(|s| s.ident.span())
                .unwrap_or_else(proc_macro2::Span::call_site);
            Some(IrExpr {
                kind: IrExprKind::Call {
                    function: func.clone(),
                    args: ir_args,
                },
                span,
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
