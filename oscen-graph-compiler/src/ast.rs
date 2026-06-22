use syn::spanned::Spanned;
use syn::{Expr, Ident};

/// Root AST node for a graph definition
// Clone is derived for the Phase 3 IR parallel path: compile() clones the
// GraphDef so lower() can consume it while the original feeds the existing
// codegen path.
#[derive(Clone)]
pub struct GraphDef {
    pub name: Option<syn::Ident>,
    pub items: Vec<GraphItem>,
}

/// Top-level items in a graph definition
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub enum GraphItem {
    Input(InputDecl),
    Output(OutputDecl),
    Node(NodeDecl),
    NodeBlock(NodeBlock),
    Connection(ConnectionStmt),
    ConnectionBlock(ConnectionBlock),
    /// `external <name>: <Type>;` - declares a runtime-bindable asset slot.
    /// The external is not a processing node: it names a graph-boundary handle
    /// that an `asset` endpoint can be bound from (`<name> -> node.asset`).
    External(ExternalDecl),
    /// `nih_params;` - enables NIH-plug parameter generation
    /// Params struct name is derived from graph name: FMGraph -> FMGraphParams
    NihParams,
    /// `name: <ident>;` declaration. Drained out of the items list into
    /// `GraphDef.name` after parsing. If a `Name` variant appears as a
    /// non-first item, the drain pass reports an error.
    Name(Ident),
}

/// Wrapper for node block to avoid orphan rule
#[derive(Clone)]
pub struct NodeBlock(pub Vec<NodeDecl>);

/// Wrapper for connection block to avoid orphan rule
#[derive(Clone)]
pub struct ConnectionBlock(pub Vec<ConnectionStmt>);

/// Input endpoint declaration
#[derive(Clone)]
pub struct InputDecl {
    pub kind: EndpointKind,
    pub name: Ident,
    pub ty: Option<syn::Type>, // Optional type annotation (e.g., [f32; 32])
    pub default: Option<Expr>,
    pub spec: Option<ParamSpec>,
}

/// `external <name>: <Type>;` declaration. Names a runtime-bindable asset slot
/// exposed at the graph boundary. The `ty` documents the asset currency
/// (e.g. `AudioAsset`); the concrete playable is resolved through the node's
/// `AssetEndpoint` impl during codegen.
#[derive(Clone)]
pub struct ExternalDecl {
    pub name: Ident,
    pub ty: syn::Type,
}

/// Output endpoint declaration
#[derive(Clone)]
pub struct OutputDecl {
    pub kind: EndpointKind,
    pub name: Ident,
    pub ty: Option<syn::Type>, // Optional type annotation (e.g., [f32; 32])
}

/// Node declaration
#[derive(Clone)]
pub struct NodeDecl {
    pub name: Ident,
    pub constructor: Expr,
    pub node_type: Option<syn::Path>,
    pub array_size: Option<usize>, // For Voice[4] syntax
    pub rate: NodeRate,
}

/// Connection statement
#[derive(Clone)]
pub struct ConnectionStmt {
    pub source: ConnectionExpr,
    pub dest: ConnectionExpr,
    pub policy: ConnectionPolicy,
    pub span: proc_macro2::Span,
    /// `Some(...)` when the user wrote `src -> [ ... ] -> dst`. Carries
    /// either a literal sample count (compiler synthesizes a hidden
    /// `Delay::new(N, 0.0)`) or a reference to a declared node (must impl
    /// `oscen::graph::AllowsFeedback`). The edge implicitly closes a
    /// feedback cycle: topo sort skips the outgoing leg of the via.
    pub via: Option<DelayVia>,
}

/// Discriminator for the contents of a `-> [ ... ] ->` bracket.
#[derive(Clone)]
pub enum DelayVia {
    /// `[N]` — compiler synthesizes an anonymous Delay node with N samples.
    Samples {
        value: syn::LitInt,
        span: proc_macro2::Span,
    },
    /// `[name]` — edge is routed through a previously declared node.
    /// Codegen emits an `AllowsFeedback` bound on the node's type.
    Node { name: syn::Ident },
}

/// Connection expression (can be endpoint, arithmetic, etc.)
#[derive(Clone)]
pub enum ConnectionExpr {
    /// Simple identifier (parameter or node name)
    Ident(Ident),
    /// Array index (e.g., voices[0])
    ArrayIndex(Box<ConnectionExpr>, usize),
    /// Field access (e.g., osc.output)
    Field(Box<ConnectionExpr>, Ident),
    /// Method call with parens (e.g., x.tanh(), x.clamp(0.0, 1.0))
    MethodCall(Box<ConnectionExpr>, Ident, Vec<Expr>),
    /// Binary operation (e.g., a * b)
    Binary(Box<ConnectionExpr>, BinaryOp, Box<ConnectionExpr>),
    /// Literal value
    Literal(Expr),
    /// Free function call (e.g., tanh(x))
    Call(Ident, Vec<ConnectionExpr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Rate ratio of a node relative to the parent graph's rate.
/// Default is `Same` (1/1). `Up(N)` means the node runs at N× the graph's rate;
/// `Down(N)` means it runs at 1/N of the graph's rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeRate {
    #[default]
    Same,
    Up(u32),   // factor must be in {2, 4, 8}
    Down(u32), // factor must be in {2, 4, 8}
}

/// Policy for a connection that crosses a rate boundary.
/// `Default` lets the macro pick based on endpoint kind (see spec § Default Policies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionPolicy {
    #[default]
    Default,
    Latch,
    Linear,
    Sinc,
    SincIir,
}

/// Endpoint type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    Stream,
    Value,
    Event,
    /// Runtime-bindable audio asset (bound from an `external`). Never resampled
    /// and imposes no processing order — handled off the cross-rate path.
    Asset,
}

/// Parameter specification (range, curve, ramp, and NIH-plug specific fields)
#[derive(Clone)]
pub struct ParamSpec {
    // Existing fields
    pub range: Option<RangeSpec>,
    pub curve: Option<Curve>,
    pub ramp: Option<usize>,
    // NIH-plug specific fields
    pub center: Option<Expr>, // Value at slider midpoint (for skewed ranges)
    pub unit: Option<String>, // Display unit (e.g., " Hz")
    pub smoother: Option<Expr>, // Smoothing time in ms
    pub step: Option<Expr>,   // Step size
    pub display_name: Option<String>, // Human-readable name (defaults to field name)
    pub group: Option<String>, // Nested params group
}

#[derive(Clone)]
pub struct RangeSpec {
    pub min: Expr,
    pub max: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Logarithmic,
}

impl ConnectionExpr {
    /// Span covering the most-meaningful token of this expression.
    /// Used by error-reporting paths that previously fell back to
    /// `Span::call_site`.
    pub fn span(&self) -> proc_macro2::Span {
        match self {
            ConnectionExpr::Ident(i) => i.span(),
            ConnectionExpr::ArrayIndex(inner, _) => inner.span(),
            ConnectionExpr::Field(inner, field) => inner
                .span()
                .join(field.span())
                .unwrap_or_else(|| inner.span()),
            ConnectionExpr::MethodCall(inner, method, _) => inner
                .span()
                .join(method.span())
                .unwrap_or_else(|| inner.span()),
            ConnectionExpr::Binary(l, _, r) => l.span().join(r.span()).unwrap_or_else(|| l.span()),
            ConnectionExpr::Literal(e) => e.span(),
            ConnectionExpr::Call(f, _) => f.span(),
        }
    }
}
