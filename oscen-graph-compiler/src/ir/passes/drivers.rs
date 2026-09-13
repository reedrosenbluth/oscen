//! Driver-plan resolution: one [`DriverGroup`] per destination slot with an
//! explicit sum / append / reject policy.
//!
//! This is where multi-driver semantics are decided. Codegen must not
//! re-derive them from endpoint syntax; it emits whatever the plan says.
//!
//! Rules, per slot with `n` drivers:
//!
//! * `n == 1` → `Single`.
//! * known **value** or **asset** kind, or any typed-value edge → `Reject`
//!   (values don't sum; the diagnostics below name both spellings).
//! * known **event** kind → `Append`; every driver must be a same-rate,
//!   scalar event edge.
//! * known **stream** kind → `Sum`; every driver must be a same-rate, simple,
//!   scalar endpoint source.
//! * **unknown** kind → the `Sum` shape (connect + accumulate, which merges
//!   for events through `AccumulateEndpoints`) plus a rustc-side
//!   `FanInAllowed` assertion on the destination's `EndpointAt` marker, so a
//!   value endpoint is still rejected — by rustc rather than the macro.
//!   Unknown never means "assume stream".
//!
//! A broadcast driver (`voices.x`) overlapping an indexed driver
//! (`voices[k].x`) gives element `k` two drivers with connection order
//! deciding; it is rejected for every kind.

use crate::ast::EndpointKind;
use crate::diagnostics::Diagnostics;
use crate::ir::drivers::{Address, DriverGroup, DriverPlan, Element, FanInPolicy, KindEvidence};
use crate::ir::expr::IrExprKind;
use crate::ir::graph::{EdgeId, EdgeKernel, EventRescale, FanoutShape, IrGraph, IrNodeKind, NodeId};
use std::collections::HashMap;

pub fn resolve(ir: &IrGraph, diags: &mut Diagnostics) -> DriverPlan {
    // Bucket by slot in canonical order; buckets keep first-seen order.
    let mut order: Vec<(NodeId, String, Option<usize>)> = Vec::new();
    let mut buckets: HashMap<(NodeId, String, Option<usize>), (Address, Vec<EdgeId>)> =
        HashMap::new();
    for &eid in &ir.edge_order {
        let addr = Address::resolve(&ir.edges[eid].dest, ir);
        let key = addr.slot_key();
        match buckets.get_mut(&key) {
            Some((_, edges)) => edges.push(eid),
            None => {
                order.push(key.clone());
                buckets.insert(key, (addr, vec![eid]));
            }
        }
    }

    let mut groups: Vec<DriverGroup> = Vec::with_capacity(order.len());
    for key in &order {
        let (dest, sources) = buckets.remove(key).expect("bucket exists");
        groups.push(classify_group(ir, dest, sources, diags));
    }

    let broadcast_of = resolve_broadcast_index_overlap(ir, &mut groups, diags);

    let mut plan = DriverPlan::default();
    for (gi, g) in groups.iter().enumerate() {
        for &eid in &g.sources {
            plan.group_of_edge.insert(eid, gi);
        }
    }
    // Emission order per node: canonical, except that a group accumulating
    // onto a broadcast is placed right after that broadcast group (and after
    // any earlier group already queued behind it).
    for (gi, g) in groups.iter().enumerate() {
        if g.onto_broadcast {
            continue;
        }
        plan.groups_by_dest_node
            .entry(g.dest.node)
            .or_default()
            .push(gi);
    }
    for (gi, g) in groups.iter().enumerate() {
        if !g.onto_broadcast {
            continue;
        }
        let bcast = broadcast_of[&gi];
        let list = plan
            .groups_by_dest_node
            .entry(g.dest.node)
            .or_default();
        let anchor = list
            .iter()
            .rposition(|&x| x == bcast || (groups[x].onto_broadcast && broadcast_of[&x] == bcast))
            .expect("broadcast group is queued before its dependents");
        list.insert(anchor + 1, gi);
    }
    plan.groups = groups;
    plan
}

fn dest_kind(ir: &IrGraph, dest: &Address) -> Option<EndpointKind> {
    ir.nodes[dest.node]
        .endpoints
        .get(&dest.field)
        .map(|ei| ei.kind)
}

fn classify_group(
    ir: &IrGraph,
    dest: Address,
    sources: Vec<EdgeId>,
    diags: &mut Diagnostics,
) -> DriverGroup {
    let kind = match dest_kind(ir, &dest) {
        Some(k) => KindEvidence::Known(k),
        None => KindEvidence::Unknown,
    };
    let has_typed = sources
        .iter()
        .any(|&e| ir.edge_is_typed_value(&ir.edges[e]));

    if sources.len() < 2 {
        return DriverGroup {
            dest,
            kind,
            sources,
            policy: FanInPolicy::Single,
            rustc_kind_check: false,
            onto_broadcast: false,
        };
    }

    let dest_desc = dest.describe(ir);
    let is_value_like = has_typed
        || matches!(
            kind,
            KindEvidence::Known(EndpointKind::Value | EndpointKind::Asset)
        );
    if is_value_like {
        for &eid in &sources[1..] {
            let msg = if has_typed {
                format!(
                    "typed value endpoint `{dest_desc}` has {} sources, but typed \
                     values cannot fan in (values don't sum); keep a single source",
                    sources.len(),
                )
            } else {
                format!(
                    "value endpoint `{dest_desc}` has {} sources, but values cannot \
                     fan in (streams sum; values don't); combine them explicitly \
                     (`a + b -> {dest_desc}`) or keep a single source",
                    sources.len(),
                )
            };
            diags.push_error(syn::Error::new(ir.edges[eid].span, msg));
        }
        return DriverGroup {
            dest,
            kind,
            sources,
            policy: FanInPolicy::Reject,
            rustc_kind_check: false,
            onto_broadcast: false,
        };
    }

    // An event bucket: the destination is a known event endpoint, or the
    // kind is unknown but a driver is an event edge (its source is a known
    // event endpoint, so the destination must be one too).
    let is_event_bucket = matches!(kind, KindEvidence::Known(EndpointKind::Event))
        || sources
            .iter()
            .any(|&e| matches!(ir.edges[e].kernel, EdgeKernel::Event { .. }));

    if is_event_bucket {
        for &eid in &sources {
            let edge = &ir.edges[eid];
            if let Some(what) = accumulate_disqualifier(ir, eid, true) {
                diags.push_error(syn::Error::new(
                    edge.span,
                    format!(
                        "event fan-in merges only same-rate scalar event sources; saw \
                         {what} into `{dest_desc}`; merge through an explicit node or \
                         keep a single source"
                    ),
                ));
            }
        }
        let unknown = matches!(kind, KindEvidence::Unknown);
        return DriverGroup {
            dest,
            kind,
            sources,
            policy: FanInPolicy::Append,
            rustc_kind_check: unknown,
            onto_broadcast: false,
        };
    }

    // Stream, or unknown: the summing shape.
    for &eid in &sources {
        let edge = &ir.edges[eid];
        if let Some(what) = accumulate_disqualifier(ir, eid, false) {
            diags.push_error(syn::Error::new(
                edge.span,
                format!(
                    "fan-in summing supports only same-rate scalar/frame stream sources; \
                     saw {what} into `{dest_desc}`"
                ),
            ));
        }
    }

    let unknown = matches!(kind, KindEvidence::Unknown);
    if unknown {
        // rustc can only veto a value endpoint through the node's
        // `EndpointAt` marker, which needs a type path to project against.
        let has_type_path = matches!(
            &ir.nodes[dest.node].kind,
            IrNodeKind::Processor { ty: Some(_), .. } | IrNodeKind::NodeArray { ty: Some(_), .. }
        );
        if !has_type_path {
            let node = &ir.nodes[dest.node].name;
            diags.push_error(syn::Error::new(
                ir.edges[sources[1]].span,
                format!(
                    "cannot verify that `{dest_desc}` accepts multiple drivers: `{node}` is \
                     not constructed from a type path, so its endpoint kinds are unknown; \
                     declare the kind through a graph input/hoist, name the node type, or \
                     keep a single source"
                ),
            ));
        }
    }

    DriverGroup {
        dest,
        kind,
        sources,
        policy: FanInPolicy::Sum,
        rustc_kind_check: unknown,
        onto_broadcast: false,
    }
}

/// Which edges disqualify a group from the accumulate shape, as a message
/// fragment; `None` when every driver is a same-rate, simple, scalar source.
fn accumulate_disqualifier(ir: &IrGraph, eid: EdgeId, event: bool) -> Option<&'static str> {
    let edge = &ir.edges[eid];
    let same_rate = matches!(
        edge.kernel,
        EdgeKernel::None
            | EdgeKernel::Event {
                rescale: EventRescale::None
            }
    );
    if !same_rate {
        return Some(if event {
            "a cross-rate event edge"
        } else {
            "a cross-rate edge"
        });
    }
    if !matches!(edge.source.kind, IrExprKind::Endpoint(_)) {
        return Some("a compound (non-endpoint) source");
    }
    match edge.fanout {
        FanoutShape::Scalar => None,
        FanoutShape::Parallel { .. } => Some("an array (parallel) source"),
        FanoutShape::Broadcast { .. } => Some("a broadcast source"),
        FanoutShape::FanIn { .. } => Some("an array fan-in source"),
    }
}

/// `voices.x` (every element) together with `voices[k].x` gives element `k`
/// two drivers. Values reject (connection order would decide which wins).
/// Streams sum and events merge, exactly as a plain fan-in would: the
/// indexed group is marked `onto_broadcast` so codegen accumulates it after
/// the broadcast. Returns, for every `onto_broadcast` group index, the index
/// of its broadcast group.
fn resolve_broadcast_index_overlap(
    ir: &IrGraph,
    groups: &mut [DriverGroup],
    diags: &mut Diagnostics,
) -> HashMap<usize, usize> {
    let mut broadcasts: HashMap<(NodeId, String), usize> = HashMap::new();
    for (gi, g) in groups.iter().enumerate() {
        if matches!(g.dest.element, Element::All { .. }) {
            broadcasts.insert((g.dest.node, g.dest.field.to_string()), gi);
        }
    }
    let mut broadcast_of = HashMap::new();
    if broadcasts.is_empty() {
        return broadcast_of;
    }
    let typed_group = |g: &DriverGroup| {
        g.sources
            .iter()
            .any(|&e| ir.edge_is_typed_value(&ir.edges[e]))
    };
    for gi in 0..groups.len() {
        let Element::Index(i) = groups[gi].dest.element else {
            continue;
        };
        let Some(&bi) = broadcasts.get(&(groups[gi].dest.node, groups[gi].dest.field.to_string()))
        else {
            continue;
        };
        let typed = typed_group(&groups[gi]) || typed_group(&groups[bi]);
        let kind = match (groups[gi].kind, groups[bi].kind) {
            (KindEvidence::Known(k), _) | (_, KindEvidence::Known(k)) => Some(k),
            _ => None,
        };
        let dest_name = &ir.nodes[groups[gi].dest.node].name;
        let endpoint = groups[gi].dest.field.clone();
        let is_value = typed || matches!(kind, Some(EndpointKind::Value | EndpointKind::Asset));
        if is_value {
            for &eid in &groups[gi].sources {
                diags.push_error(syn::Error::new(
                    ir.edges[eid].span,
                    format!(
                        "value endpoint `{dest_name}[{i}].{endpoint}` is driven both directly \
                         and by a broadcast connection to `{dest_name}.{endpoint}` (values \
                         cannot fan in); drop one of the two drivers",
                    ),
                ));
            }
            groups[gi].policy = FanInPolicy::Reject;
            continue;
        }
        let event = matches!(kind, Some(EndpointKind::Event))
            || groups[gi]
                .sources
                .iter()
                .chain(groups[bi].sources.iter())
                .any(|&e| matches!(ir.edges[e].kernel, EdgeKernel::Event { .. }));
        // The broadcast itself is written first (any same-rate source shape
        // is fine there); the indexed drivers accumulate onto it, so they
        // need the plain scalar-endpoint shape.
        let mut ok = true;
        let checks = groups[gi]
            .sources
            .iter()
            .map(|&e| (e, false))
            .chain(groups[bi].sources.iter().map(|&e| (e, true)))
            .collect::<Vec<_>>();
        for (eid, is_broadcast_side) in checks {
            let problem = if is_broadcast_side {
                let same_rate = matches!(
                    ir.edges[eid].kernel,
                    EdgeKernel::None
                        | EdgeKernel::Event {
                            rescale: EventRescale::None
                        }
                );
                (!same_rate).then_some("a cross-rate broadcast")
            } else {
                accumulate_disqualifier(ir, eid, event)
            };
            if let Some(what) = problem {
                ok = false;
                diags.push_error(syn::Error::new(
                    ir.edges[eid].span,
                    format!(
                        "`{dest_name}[{i}].{endpoint}` is driven both directly and by a \
                         broadcast connection to `{dest_name}.{endpoint}`; the element {}s \
                         both, which needs same-rate scalar endpoint sources, but saw {what}; \
                         drop one of the two drivers or connect every element explicitly",
                        if event { "merge" } else { "sum" },
                    ),
                ));
            }
        }
        if !ok {
            continue;
        }
        let unknown = kind.is_none();
        let g = &mut groups[gi];
        g.onto_broadcast = true;
        g.policy = if event {
            FanInPolicy::Append
        } else {
            FanInPolicy::Sum
        };
        g.rustc_kind_check = unknown;
        broadcast_of.insert(gi, bi);
    }
    broadcast_of
}
