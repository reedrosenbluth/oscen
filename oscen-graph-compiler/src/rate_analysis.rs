use crate::ast::{
    ConnectionPolicy, ConnectionStmt, EndpointKind, GraphDef, GraphItem, NodeDecl, NodeRate,
};
use crate::fanout::{classify_fanout, FanoutShape};
use crate::type_check::TypeContext;
use std::collections::HashMap;
use syn::Result;

/// Per-edge frame_offset rescaling for event-typed cross-rate edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRescale {
    /// Same-rate edge: no rescaling applied.
    None,
    /// Outer -> inner: multiply offsets by N.
    Multiply(u32),
    /// Inner -> outer: divide offsets by N.
    Divide(u32),
}

/// Resampling kernel selection for a single cross-rate edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKernel {
    /// No conversion needed (same rate, or both directions are no-op).
    None,
    /// Upsample: source slower, dest faster.
    Up { factor: u32, kind: ConnectionPolicy },
    /// Downsample: source faster, dest slower.
    Down { factor: u32, kind: ConnectionPolicy },
    /// Event-typed edge. Same-rate (`rescale = None`) is functionally
    /// equivalent to `EdgeKernel::None` and emits a plain copy via the
    /// existing event try_push path; cross-rate variants emit the same
    /// try_push loop but transform `EventInstance::frame_offset` per
    /// `rescale` so events fire on the correct inner/outer tick.
    Event { rescale: EventRescale },
}

/// Per-edge analysis result. `edge_index` indexes into the original `connections` slice.
#[derive(Debug, Clone)]
pub struct EdgeRate {
    pub edge_index: usize,
    pub source_rate: NodeRate,
    pub dest_rate: NodeRate,
    pub kernel: EdgeKernel,
    /// Per-edge fan-out shape derived from source/dest node array sizes.
    /// Used by codegen to dispatch between scalar / parallel / broadcast /
    /// fan-in emission for both same-rate and cross-rate edges.
    pub shape: FanoutShape,
}

/// Per-graph rate analysis.
#[derive(Debug, Clone)]
pub struct RateAnalysis {
    /// Node name → rate.
    pub node_rates: HashMap<String, NodeRate>,
    /// `lcm` of all node up-factors (1 if everything is at outer rate).
    pub max_factor: u32,
    /// `lcm` of all node down-divisors.
    pub min_divisor: u32,
    /// One entry per connection, in original order.
    pub edges: Vec<EdgeRate>,
}

#[derive(Copy, Clone)]
enum Direction {
    Up,
    Down,
}

/// Analyze a parsed graph. Validates rates and produces edge classifications.
pub fn analyze(def: &GraphDef) -> Result<RateAnalysis> {
    // 1. Collect all node declarations.
    let mut nodes: Vec<&NodeDecl> = Vec::new();
    for item in &def.items {
        match item {
            GraphItem::Node(n) => nodes.push(n),
            GraphItem::NodeBlock(b) => nodes.extend(b.0.iter()),
            _ => {}
        }
    }

    let mut node_rates = HashMap::new();
    let mut node_array_sizes: HashMap<String, Option<usize>> = HashMap::new();
    let mut max_factor: u32 = 1;
    let mut min_divisor: u32 = 1;
    for n in &nodes {
        // v1 scope: only oversampling (`* N`) is implemented. Reject `/ N`
        // (undersampling) here so users get a clear error at macro-expansion
        // time instead of a confusing codegen failure.
        if let NodeRate::Down(_) = n.rate {
            return Err(syn::Error::new(
                n.name.span(),
                "node undersampling (`/ N`) is not yet supported in v1; only oversampling (`* N`) is implemented",
            ));
        }
        node_rates.insert(n.name.to_string(), n.rate);
        node_array_sizes.insert(n.name.to_string(), n.array_size);
        match n.rate {
            NodeRate::Up(f) => {
                max_factor = lcm(max_factor, f);
            }
            NodeRate::Down(d) => {
                // Rejected above; kept for future when undersampling is added.
                min_divisor = lcm(min_divisor, d);
            }
            NodeRate::Same => {}
        }
    }

    // 2. Collect all connections.
    let mut conns: Vec<&ConnectionStmt> = Vec::new();
    for item in &def.items {
        match item {
            GraphItem::Connection(c) => conns.push(c),
            GraphItem::ConnectionBlock(b) => conns.extend(b.0.iter()),
            _ => {}
        }
    }

    // 3. Classify each edge.
    //
    // This first pass classifies edges purely from rate annotations. The
    // resulting `EdgeKernel`s for cross-rate edges are stream-typed (Up/Down
    // with kernel chosen by policy) and may use `ConnectionPolicy::Default`.
    // Node-to-node event edges and value vs. stream default-policy resolution
    // need endpoint-kind metadata; those refinements happen in
    // `refine_with_types`, called after the codegen's TypeContext is built.
    let mut edges = Vec::with_capacity(conns.len());
    for (idx, c) in conns.iter().enumerate() {
        let src_node = root_node_name(&c.source);
        let dst_node = root_node_name(&c.dest);
        let source_rate = src_node
            .as_ref()
            .and_then(|n| node_rates.get(n).copied())
            .unwrap_or(NodeRate::Same);
        let dest_rate = dst_node
            .as_ref()
            .and_then(|n| node_rates.get(n).copied())
            .unwrap_or(NodeRate::Same);

        let kernel = classify_edge(source_rate, dest_rate, c.policy, c.span)?;

        // Resolve array sizes through the same root-name keying as rates.
        // Indexed accesses (`voices[3].field`) deliberately use the array
        // size of the parent — current codegen treats them as fan-in/out
        // by sum at same-rate; cross-rate indexed access remains out of
        // scope for v1 (see spec §"Out of Scope").
        let src_size = src_node
            .as_ref()
            .and_then(|n| node_array_sizes.get(n).copied())
            .unwrap_or(None);
        let dst_size = dst_node
            .as_ref()
            .and_then(|n| node_array_sizes.get(n).copied())
            .unwrap_or(None);
        let shape = classify_fanout(src_size, dst_size);

        edges.push(EdgeRate {
            edge_index: idx,
            source_rate,
            dest_rate,
            kernel,
            shape,
        });
    }

    Ok(RateAnalysis {
        node_rates,
        max_factor,
        min_divisor,
        edges,
    })
}

/// Refine the rate analysis using endpoint-kind information from the type
/// context built by codegen. Two refinements run here:
///
/// 1. **Event edges.** Any edge whose source OR destination endpoint is an
///    event endpoint is rewritten to `EdgeKernel::Event` with rescaling
///    derived from the source/dest rates. This covers both graph-level event
///    endpoints (which used to be special-cased via a name-based hack) and
///    node-to-node event edges (previously broken — they classified as
///    stream Up/Down and emitted code that didn't type-check).
///
/// 2. **Default policy on value edges.** A cross-rate edge whose source is a
///    value endpoint and whose policy was left as `ConnectionPolicy::Default`
///    is rewritten to `ConnectionPolicy::Latch`. Stream edges keep their
///    Sinc default (resolved later in codegen `kernel_*_type` helpers).
pub fn refine_with_types(
    rate_analysis: &mut RateAnalysis,
    conns: &[ConnectionStmt],
    type_ctx: &TypeContext,
) {
    for edge in rate_analysis.edges.iter_mut() {
        let conn = &conns[edge.edge_index];
        let src_kind = type_ctx.infer_type(&conn.source);
        let dst_kind = type_ctx.infer_type(&conn.dest);

        let is_event_edge = matches!(src_kind, Some(EndpointKind::Event))
            || matches!(dst_kind, Some(EndpointKind::Event));

        if is_event_edge {
            // Determined event edge: rescale frame_offset across rate boundaries.
            let rescale = event_rescale(edge.source_rate, edge.dest_rate);
            edge.kernel = EdgeKernel::Event { rescale };
            continue;
        }

        // Default policy on value cross-rate edges should latch, not sinc.
        let is_value_edge = matches!(src_kind, Some(EndpointKind::Value))
            || matches!(dst_kind, Some(EndpointKind::Value));
        if is_value_edge {
            match &mut edge.kernel {
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

/// Compute the `EventRescale` for a single event edge given source/dest rates.
fn event_rescale(src: NodeRate, dst: NodeRate) -> EventRescale {
    use NodeRate::*;
    match (src, dst) {
        (Same, Up(n)) => EventRescale::Multiply(n),
        (Up(n), Same) => EventRescale::Divide(n),
        // Same -> Same, Up(n) -> Up(n): no rescaling needed.
        _ => EventRescale::None,
    }
}

fn classify_edge(
    src: NodeRate,
    dst: NodeRate,
    policy: ConnectionPolicy,
    span: proc_macro2::Span,
) -> Result<EdgeKernel> {
    use NodeRate::*;
    let (factor, direction) = match (src, dst) {
        (Same, Same) => return Ok(EdgeKernel::None),
        (Up(n), Same) => (n, Direction::Down),
        (Same, Up(n)) => (n, Direction::Up),
        (Same, Down(n)) => (n, Direction::Down),
        (Down(n), Same) => (n, Direction::Up),
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

    Ok(match direction {
        Direction::Up => EdgeKernel::Up {
            factor,
            kind: policy,
        },
        Direction::Down => EdgeKernel::Down {
            factor,
            kind: policy,
        },
    })
}

/// Extract the root node name from a connection expression (the leftmost identifier).
pub(crate) fn root_node_name(expr: &crate::ast::ConnectionExpr) -> Option<String> {
    use crate::ast::ConnectionExpr::*;
    match expr {
        Ident(i) => Some(i.to_string()),
        Field(inner, _) => root_node_name(inner),
        ArrayIndex(inner, _) => root_node_name(inner),
        MethodCall(inner, _, _) => root_node_name(inner),
        Binary(_, _, _) | Literal(_) | Call(_, _) => None,
    }
}

fn lcm(a: u32, b: u32) -> u32 {
    a / gcd(a, b) * b
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Cross-rate edges support a fixed set of `(SrcKind, DstKind)` tuples.
/// Anything else has no `CrossRateKernel` impl and would have produced a
/// confusing trait-resolution error pointed at the macro block; the
/// `graph!` macro refuses these explicitly with a span at the connection.
pub(crate) fn is_supported_cross_rate_kinds(src: EndpointKind, dst: EndpointKind) -> bool {
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

/// Walk the rate-analysis edges and return the first `syn::Error` for an
/// unsupported cross-rate kind tuple, or `Ok(())` if all edges are valid.
/// Edges where one or both kinds cannot be inferred from `type_ctx` are
/// skipped — those produce errors elsewhere or are not cross-rate.
pub(crate) fn validate_cross_rate_kinds(
    rate_analysis: &RateAnalysis,
    connections: &[ConnectionStmt],
    type_ctx: &TypeContext,
) -> syn::Result<()> {
    for edge in &rate_analysis.edges {
        let is_cross_rate = matches!(
            edge.kernel,
            EdgeKernel::Up { .. } | EdgeKernel::Down { .. }
        );
        if !is_cross_rate {
            continue;
        }
        let conn = &connections[edge.edge_index];
        let (src, dst) = match (type_ctx.infer_type(&conn.source), type_ctx.infer_type(&conn.dest))
        {
            (Some(s), Some(d)) => (s, d),
            _ => continue,
        };
        if is_supported_cross_rate_kinds(src, dst) {
            continue;
        }
        return Err(syn::Error::new(
            conn.span,
            format!(
                "cross-rate edge from {} to {} is not supported; \
                 insert an explicit converter node, or change one side's rate",
                endpoint_kind_name(src),
                endpoint_kind_name(dst),
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::fanout::FanoutShape;
    use super::*;
    use syn::parse_quote;

    fn parse(src: proc_macro2::TokenStream) -> crate::ast::GraphDef {
        syn::parse2(src).expect("parse failed")
    }

    #[test]
    fn analyze_classifies_scalar_to_array_as_broadcast() {
        let def = parse(parse_quote! {
            name: G;
            input value v = 0.0;
            nodes {
                xs = [Holder::new(); 4];
            }
            connections {
                v -> xs.input;
            }
        });
        let ra = analyze(&def).expect("analyze failed");
        assert_eq!(ra.edges.len(), 1);
        assert_eq!(ra.edges[0].shape, FanoutShape::Broadcast { n: 4 });
    }

    #[test]
    fn analyze_classifies_array_to_array_as_parallel() {
        let def = parse(parse_quote! {
            name: G;
            nodes {
                xs = [Src::new(); 4];
                ys = [Dst::new(); 4];
            }
            connections {
                xs.out -> ys.input;
            }
        });
        let ra = analyze(&def).expect("analyze failed");
        assert_eq!(ra.edges[0].shape, FanoutShape::Parallel { n: 4 });
    }

    #[test]
    fn analyze_classifies_array_to_scalar_as_fanin() {
        let def = parse(parse_quote! {
            name: G;
            output stream o;
            nodes {
                xs = [Src::new(); 8];
            }
            connections {
                xs.out -> o;
            }
        });
        let ra = analyze(&def).expect("analyze failed");
        assert_eq!(ra.edges[0].shape, FanoutShape::FanIn { n: 8 });
    }
}

#[cfg(test)]
mod cross_rate_kind_tests {
    use super::is_supported_cross_rate_kinds;
    use crate::ast::EndpointKind;

    #[test]
    fn supported_tuples_are_supported() {
        assert!(is_supported_cross_rate_kinds(EndpointKind::Stream, EndpointKind::Stream));
        assert!(is_supported_cross_rate_kinds(EndpointKind::Value, EndpointKind::Value));
        assert!(is_supported_cross_rate_kinds(EndpointKind::Value, EndpointKind::Stream));
        assert!(is_supported_cross_rate_kinds(EndpointKind::Event, EndpointKind::Event));
    }

    #[test]
    fn unsupported_tuples_are_unsupported() {
        assert!(!is_supported_cross_rate_kinds(EndpointKind::Event, EndpointKind::Stream));
        assert!(!is_supported_cross_rate_kinds(EndpointKind::Stream, EndpointKind::Event));
        assert!(!is_supported_cross_rate_kinds(EndpointKind::Stream, EndpointKind::Value));
        assert!(!is_supported_cross_rate_kinds(EndpointKind::Event, EndpointKind::Value));
        assert!(!is_supported_cross_rate_kinds(EndpointKind::Value, EndpointKind::Event));
    }
}
