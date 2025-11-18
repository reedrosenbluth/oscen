use syn::{Expr, Ident};

/// Root AST node for a graph definition
pub struct GraphDef {
    pub name: Option<syn::Ident>,
    pub compile_time: bool,
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
}

/// Wrapper for node block to avoid orphan rule
pub struct NodeBlock(pub Vec<NodeDecl>);

/// Wrapper for connection block to avoid orphan rule
pub struct ConnectionBlock(pub Vec<ConnectionStmt>);

/// Input endpoint declaration
#[allow(dead_code)]
pub struct InputDecl {
    pub kind: EndpointKind,
    pub name: Ident,
    pub ty: Option<syn::Type>, // Optional type annotation (e.g., [f32; 32])
    pub default: Option<Expr>,
    pub spec: Option<ParamSpec>,
}

/// Output endpoint declaration
pub struct OutputDecl {
    pub kind: EndpointKind,
    pub name: Ident,
    pub ty: Option<syn::Type>, // Optional type annotation (e.g., [f32; 32])
}

/// Node declaration
pub struct NodeDecl {
    pub name: Ident,
    pub constructor: Expr,
    pub node_type: Option<syn::Path>,
    pub array_size: Option<usize>, // For Voice[4] syntax
}

/// Connection statement
pub struct ConnectionStmt {
    pub source: ConnectionExpr,
    pub dest: ConnectionExpr,
}

/// Connection expression (can be endpoint, arithmetic, etc.)
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

/// Parameter specification (range, curve, ramp)
#[allow(dead_code)]
pub struct ParamSpec {
    pub range: Option<RangeSpec>,
    pub curve: Option<Curve>,
    pub ramp: Option<usize>,
}

#[allow(dead_code)]
pub struct RangeSpec {
    pub min: Expr,
    pub max: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Logarithmic,
}
