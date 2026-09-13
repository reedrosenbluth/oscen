//! Resolved per-destination driver plan.
//!
//! Every connection ends at one *slot*: a node endpoint, one element of a
//! node-array endpoint, or a graph output. The lowering pass in
//! `passes::drivers` groups every edge by the slot it drives and decides,
//! once per slot, what several drivers mean: stream slots **sum**, event
//! slots **merge** (append), value slots take exactly one driver. Codegen
//! consumes the resulting [`DriverPlan`] mechanically instead of
//! re-deriving fan-in semantics from the endpoint's syntax.
//!
//! [`Address`] is the resolved form of an [`IrEndpoint`]: it separates
//! *which element* of a node array is addressed from *which channel* of a
//! frame is selected, so every emitter agrees on what `voices[2].input`
//! means.

use crate::ast::EndpointKind;
use crate::ir::expr::IrEndpoint;
use crate::ir::graph::{EdgeId, IrGraph, IrNodeKind, NodeId};
use proc_macro2::Span;
use slotmap::SecondaryMap;
use std::collections::HashMap;
use syn::Ident;

/// Which node element an address selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// A scalar node (or a graph endpoint).
    Scalar,
    /// Every element of a node array of length `n` (broadcast / parallel).
    All { n: usize },
    /// One element of a node array.
    Index(usize),
}

/// A resolved endpoint address.
#[derive(Clone, Debug)]
pub struct Address {
    pub node: NodeId,
    /// Endpoint name. For a bare graph input/output reference this equals
    /// the node name and `bare` is set; emission then uses `self.<node>`.
    pub field: Ident,
    pub bare: bool,
    pub element: Element,
    /// A channel index on a *scalar* node's frame-typed endpoint
    /// (`s.output[0]`). Only sources carry channel selections today.
    pub channel: Option<usize>,
    pub span: Span,
}

impl Address {
    /// Resolve an `IrEndpoint` against the graph: an index on a node array
    /// selects an element, an index on a scalar node selects a channel.
    pub fn resolve(ep: &IrEndpoint, ir: &IrGraph) -> Address {
        let array_len = match &ir.nodes[ep.node].kind {
            IrNodeKind::NodeArray { len, .. } => Some(*len),
            _ => None,
        };
        let (element, channel) = match (array_len, ep.index) {
            (Some(_), Some(k)) => (Element::Index(k), None),
            (Some(n), None) => (Element::All { n }, None),
            (None, Some(c)) => (Element::Scalar, Some(c)),
            (None, None) => (Element::Scalar, None),
        };
        Address {
            node: ep.node,
            field: ep.endpoint.clone(),
            bare: ep.bare,
            element,
            channel,
            span: ep.span,
        }
    }

    /// The slot this address drives. Element index participates (each array
    /// element is its own slot); an `All` broadcast and an `Index(k)` on the
    /// same endpoint are different keys and are checked for overlap by the
    /// driver pass.
    pub fn slot_key(&self) -> (NodeId, String, Option<usize>) {
        let idx = match self.element {
            Element::Index(k) => Some(k),
            _ => None,
        };
        (self.node, self.field.to_string(), idx)
    }

    /// Human-readable spelling for diagnostics: `out`, `flt.input`,
    /// `voices[2].input`.
    pub fn describe(&self, ir: &IrGraph) -> String {
        let node = &ir.nodes[self.node].name;
        if self.bare {
            return node.to_string();
        }
        match self.element {
            Element::Index(k) => format!("{node}[{k}].{}", self.field),
            _ => format!("{node}.{}", self.field),
        }
    }
}

/// What several drivers into one slot mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanInPolicy {
    /// Exactly one driver: a plain connect.
    Single,
    /// Stream slot: `connect` the first driver, `accumulate` (sum) the rest.
    Sum,
    /// Event slot: `connect` the first driver, `accumulate` (append) the rest.
    Append,
    /// Value/asset slot with several drivers: rejected during lowering.
    /// Never reaches codegen.
    Reject,
}

/// How much the compiler knows about the destination endpoint's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindEvidence {
    /// Declared on a graph endpoint or inferred through a connection to one.
    Known(EndpointKind),
    /// A node endpoint never anchored to a typed graph endpoint. Only the
    /// node's `EndpointAt` marker (rustc) can tell its kind.
    Unknown,
}

/// All drivers of one slot.
#[derive(Clone, Debug)]
pub struct DriverGroup {
    pub dest: Address,
    pub kind: KindEvidence,
    /// Driving edges in canonical `edge_order`.
    pub sources: Vec<EdgeId>,
    pub policy: FanInPolicy,
    /// True when the group has several drivers and the destination kind is
    /// unknown at macro time: codegen emits a `FanInAllowed` assertion on
    /// the destination's `EndpointAt::Kind` so rustc rejects a value slot.
    pub rustc_kind_check: bool,
    /// This `Index(k)` group shares its endpoint with an `All` (broadcast)
    /// group: element `k` is driven by both. Codegen emits this group after
    /// the broadcast and accumulates *every* driver (no leading `connect`),
    /// so the element ends up with the sum / merged events of both.
    pub onto_broadcast: bool,
}

impl DriverGroup {
    /// More than one driver, or a single driver joining a broadcast: either
    /// way the emitted shape accumulates.
    pub fn accumulates(&self) -> bool {
        self.sources.len() > 1 || self.onto_broadcast
    }
}

/// The resolved driver plan for a graph. Built by `passes::drivers::resolve`
/// after lowering (and rebuilt after any pass that removes edges).
#[derive(Default)]
pub struct DriverPlan {
    /// Groups ordered by the canonical position of their first driver.
    pub groups: Vec<DriverGroup>,
    /// Index into `groups` for every live edge.
    pub group_of_edge: SecondaryMap<EdgeId, usize>,
    /// Indices into `groups`, per destination node, in emission order: group
    /// order, except that an `onto_broadcast` group follows the broadcast
    /// group it accumulates onto.
    pub groups_by_dest_node: HashMap<NodeId, Vec<usize>>,
}

impl DriverPlan {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn group_for_edge(&self, edge: EdgeId) -> Option<&DriverGroup> {
        self.group_of_edge.get(edge).map(|&i| &self.groups[i])
    }

    /// Groups whose destination is `node`, in canonical order.
    pub fn groups_for_node(&self, node: NodeId) -> impl Iterator<Item = &DriverGroup> + '_ {
        let groups = &self.groups;
        self.groups_by_dest_node
            .get(&node)
            .into_iter()
            .flat_map(move |ixs| ixs.iter().map(move |&i| &groups[i]))
    }
}
