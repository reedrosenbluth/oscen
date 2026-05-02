use super::ast::{ConnectionPolicy, ConnectionStmt, GraphDef, GraphItem, NodeDecl, NodeRate};
use std::collections::HashMap;
use syn::Result;

/// Resampling kernel selection for a single cross-rate edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKernel {
    /// No conversion needed (same rate, or both directions are no-op).
    None,
    /// Upsample: source slower, dest faster.
    Up { factor: u32, kind: ConnectionPolicy },
    /// Downsample: source faster, dest slower.
    Down { factor: u32, kind: ConnectionPolicy },
}

/// Per-edge analysis result. `edge_index` indexes into the original `connections` slice.
#[derive(Debug, Clone)]
pub struct EdgeRate {
    pub edge_index: usize,
    pub source_rate: NodeRate,
    pub dest_rate: NodeRate,
    pub kernel: EdgeKernel,
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

        // Span for error reporting: prefer source ident, fall back to call_site.
        let span = match &c.source {
            super::ast::ConnectionExpr::Ident(i) => i.span(),
            _ => proc_macro2::Span::call_site(),
        };

        let kernel = classify_edge(source_rate, dest_rate, c.policy, span)?;
        edges.push(EdgeRate { edge_index: idx, source_rate, dest_rate, kernel });
    }

    Ok(RateAnalysis { node_rates, max_factor, min_divisor, edges })
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
        Direction::Up => EdgeKernel::Up { factor, kind: policy },
        Direction::Down => EdgeKernel::Down { factor, kind: policy },
    })
}

/// Extract the root node name from a connection expression (the leftmost identifier).
fn root_node_name(expr: &super::ast::ConnectionExpr) -> Option<String> {
    use super::ast::ConnectionExpr::*;
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
