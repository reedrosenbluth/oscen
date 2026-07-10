use std::collections::HashSet;
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

impl GraphDef {
    /// Every node declaration — top-level and inside `nodes {}` blocks —
    /// in declaration order.
    pub fn node_decls(&self) -> impl Iterator<Item = &NodeDecl> {
        self.items.iter().flat_map(|item| match item {
            GraphItem::Node(n) => std::slice::from_ref(n).iter(),
            GraphItem::NodeBlock(b) => b.0.iter(),
            _ => [].iter(),
        })
    }

    /// The name of every node declaration (see [`Self::node_decls`]).
    pub fn node_decl_names(&self) -> HashSet<String> {
        self.node_decls().map(|n| n.name.to_string()).collect()
    }

    /// The name claimed by every named declaration: inputs (by declared
    /// name; endpoint-list hoists claim one rename-applied name per
    /// endpoint), outputs, externals, and nodes. Wildcard-hoist inputs
    /// contribute nothing — their `InputDecl.name` is a parse placeholder
    /// and the names they expand to aren't known until manifest
    /// resolution.
    pub fn declared_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        for item in &self.items {
            match item {
                GraphItem::Input(input) => match &input.hoist {
                    Some(HoistSource {
                        endpoints: HoistEndpoints::Wildcard { .. },
                        ..
                    }) => {}
                    Some(HoistSource {
                        endpoints: HoistEndpoints::List { endpoints, rename },
                        ..
                    }) => {
                        for ep in endpoints {
                            let name = match rename {
                                Some(pat) => pat.apply_str(ep),
                                None => ep.to_string(),
                            };
                            names.insert(name);
                        }
                    }
                    _ => {
                        names.insert(input.name.to_string());
                    }
                },
                GraphItem::Output(output) => {
                    names.insert(output.name.to_string());
                }
                GraphItem::External(ext) => {
                    names.insert(ext.name.to_string());
                }
                GraphItem::Node(n) => {
                    names.insert(n.name.to_string());
                }
                GraphItem::NodeBlock(b) => {
                    for n in &b.0 {
                        names.insert(n.name.to_string());
                    }
                }
                GraphItem::Connection(_)
                | GraphItem::ConnectionBlock(_)
                | GraphItem::NihParams
                | GraphItem::Name(_) => {}
            }
        }
        names
    }
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
    /// `Some` when this input hoists a child node endpoint
    /// (`input voices.cutoff;`): the compiler declares the graph input *and*
    /// synthesizes the `name -> node.endpoint` connection. Value hoists
    /// without an explicit `= default` inherit their initial value from the
    /// constructed child node at runtime.
    pub hoist: Option<HoistSource>,
}

/// The `node.endpoint` path of a hoisted endpoint declaration.
#[derive(Clone)]
pub struct HoistSource {
    pub node: Ident,
    pub endpoints: HoistEndpoints,
}

impl HoistSource {
    /// The single hoisted endpoint, when this is a single-endpoint hoist.
    /// List hoists are expanded into singles during lowering (step 0), and
    /// wildcard hoists are expanded into singles before lowering (after
    /// manifest resolution), so IR consumers (codegen default-inheritance)
    /// only ever see `Single`.
    pub fn single_endpoint(&self) -> Option<&Ident> {
        match &self.endpoints {
            HoistEndpoints::Single(ep) => Some(ep),
            HoistEndpoints::List { .. } | HoistEndpoints::Wildcard { .. } => None,
        }
    }
}

/// Endpoint selection of a hoist declaration.
#[derive(Clone)]
pub enum HoistEndpoints {
    /// `input voices.cutoff;` — one endpoint; the graph input name (after
    /// any rename) lives in `InputDecl.name`.
    Single(Ident),
    /// `input branch_a.{attack, decay} env_a_*;` — several endpoints hoisted
    /// in one declaration, optionally renamed through a `*`-substitution
    /// pattern. Expanded to `Single` hoists during lowering.
    List {
        endpoints: Vec<Ident>,
        rename: Option<RenamePattern>,
    },
    /// `input voices.*;` — hoist every input endpoint of the node. The
    /// endpoint set comes from the node type's exported manifest macro
    /// (`__oscen_endpoints_<Type>!`), resolved through a two-stage
    /// continuation-passing expansion in `oscen-macros`; the compiler
    /// substitutes the resolved endpoints as a `List` hoist before
    /// lowering. No rename pattern and no default/spec are allowed.
    ///
    /// `span` covers the `*` token so diagnostics about the wildcard
    /// point at the parent's `input node.*;` statement, never at
    /// child-crate manifest tokens.
    Wildcard { span: proc_macro2::Span },
}

/// A `prefix*suffix` rename pattern: `*` is replaced by the endpoint name.
#[derive(Clone)]
pub struct RenamePattern {
    pub prefix: String,
    pub suffix: String,
    pub span: proc_macro2::Span,
}

impl RenamePattern {
    /// The renamed endpoint as a string. Raw-ident endpoints (`r#loop`)
    /// contribute their bare name (`loop`), so `env_*` on `r#loop` yields
    /// `env_loop`, never the invalid `env_r#loop`.
    pub fn apply_str(&self, endpoint: &Ident) -> String {
        format!("{}{}{}", self.prefix, ident_base(endpoint), self.suffix)
    }

    /// The renamed endpoint as an ident, or a spanned error when the result
    /// cannot be an identifier (e.g. a pattern that produces `Self`).
    pub fn try_apply(&self, endpoint: &Ident) -> syn::Result<Ident> {
        let name = self.apply_str(endpoint);
        make_ident(&name, endpoint.span()).ok_or_else(|| {
            syn::Error::new(
                self.span,
                format!(
                    "rename pattern produces `{name}` for endpoint `{endpoint}`, \
                     which cannot be used as an identifier; choose a different \
                     prefix/suffix"
                ),
            )
        })
    }
}

/// An ident's name without any raw-ident prefix.
fn ident_base(ident: &Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").map(str::to_owned).unwrap_or(s)
}

/// Build an ident from an arbitrary string: plain idents pass through,
/// keywords become raw idents, and the handful of names that cannot be raw
/// (`crate`, `self`, `Self`, `super`, `_`) yield `None`.
pub(crate) fn make_ident(name: &str, span: proc_macro2::Span) -> Option<Ident> {
    if syn::parse_str::<Ident>(name).is_ok() {
        Some(Ident::new(name, span))
    } else if matches!(name, "crate" | "self" | "Self" | "super" | "_") || name.is_empty() {
        None
    } else if syn::parse_str::<Ident>(&format!("r#{name}")).is_ok() {
        Some(Ident::new_raw(name, span))
    } else {
        None
    }
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
    /// Free function call (e.g., `tanh(x)`, `dsp::decode_ms(x)`). The name is a
    /// path so it can be module-qualified.
    Call(syn::Path, Vec<ConnectionExpr>),
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
            ConnectionExpr::Call(f, _) => f
                .segments
                .last()
                .map(|s| s.ident.span())
                .unwrap_or_else(proc_macro2::Span::call_site),
        }
    }
}
