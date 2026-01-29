use syn::{Expr, Ident};

/// Root AST node for a graph definition
pub struct GraphDef {
    pub name: Option<syn::Ident>,
    pub items: Vec<GraphItem>,
}

/// Top-level items in a graph definition
pub enum GraphItem {
    Input(InputDecl),
    Output(OutputDecl),
    Node(NodeDecl),
    NodeBlock(NodeBlock),
    Connection(ConnectionStmt),
    ConnectionBlock(ConnectionBlock),
    /// `nih_params;` - enables NIH-plug parameter generation
    /// Params struct name is derived from graph name: FMGraph -> FMGraphParams
    NihParams,
}

/// Wrapper for node block to avoid orphan rule
pub struct NodeBlock(pub Vec<NodeDecl>);

/// Wrapper for connection block to avoid orphan rule
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
}

/// Connection statement
#[derive(Clone)]
pub struct ConnectionStmt {
    pub source: ConnectionExpr,
    pub dest: ConnectionExpr,
}

/// Connection expression (can be endpoint, arithmetic, etc.)
#[derive(Clone)]
pub enum ConnectionExpr {
    /// Simple identifier (parameter or node name)
    Ident(Ident),
    /// Array index (e.g., voices[0])
    ArrayIndex(Box<ConnectionExpr>, usize),
    /// Method call (e.g., filter.cutoff())
    Method(Box<ConnectionExpr>, Ident, Vec<Expr>),
    /// Binary operation (e.g., a * b)
    Binary(Box<ConnectionExpr>, BinaryOp, Box<ConnectionExpr>),
    /// Literal value
    Literal(Expr),
    /// Function call
    Call(Ident, Vec<ConnectionExpr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Endpoint type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    Stream,
    Value,
    Event,
}

/// Parameter specification (range, curve, ramp, and NIH-plug specific fields)
#[derive(Clone)]
pub struct ParamSpec {
    // Existing fields
    pub range: Option<RangeSpec>,
    pub curve: Option<Curve>,
    pub ramp: Option<usize>,
    // NIH-plug specific fields
    pub center: Option<Expr>,          // Value at slider midpoint (for skewed ranges)
    pub unit: Option<String>,          // Display unit (e.g., " Hz")
    pub smoother: Option<Expr>,        // Smoothing time in ms
    pub step: Option<Expr>,            // Step size
    pub display_name: Option<String>,  // Human-readable name (defaults to field name)
    pub group: Option<String>,         // Nested params group
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
