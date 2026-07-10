//! Endpoint-manifest support for wildcard hoists (`input voices.*;`).
//!
//! A proc macro cannot enumerate another type's endpoints, so every
//! `#[derive(Node)]` type and every generated graph exports a "manifest"
//! `macro_rules!` macro (`__oscen_endpoints_<TypeName>!`) carrying its
//! endpoint list. When a `graph!` body contains wildcard hoists, the macro
//! expands to a manifest invocation in continuation-passing style; the
//! continuation (`__oscen_graph_resume` in `oscen-macros`) collects each
//! node's manifest and, once all are resolved, re-enters the compiler via
//! [`crate::compile_with_manifests`].
//!
//! This module provides the pieces both sides need:
//!
//! - [`scan_wildcards`]: parse a `graph!` body and report which nodes are
//!   wildcard-hoisted, with the manifest macro path for each.
//! - [`NodeManifest`]: the parsed form of one manifest's token payload.
//! - [`expand_wildcards`]: substitute resolved manifests into explicit
//!   single-endpoint hoists (the existing Phase-2 machinery) before
//!   lowering, applying the skip rules.

use crate::ast::{
    ConnectionExpr, EndpointKind, GraphDef, GraphItem, HoistEndpoints, HoistSource, InputDecl,
    NodeDecl,
};
use crate::codegen::helpers::ident_base;
use crate::diagnostics::Diagnostics;
use proc_macro2::{Span, TokenStream};
use std::collections::{HashMap, HashSet};
use syn::parse::{Parse, ParseStream};
use syn::{bracketed, Ident, Token};

/// One wildcard hoist statement found in a `graph!` body: the node it
/// names and the path of the manifest macro that can enumerate the node
/// type's endpoints.
pub struct WildcardRequest {
    /// The hoisted node's name (`voices` in `input voices.*;`).
    pub node: Ident,
    /// Path of the manifest macro: the node type's path with the last
    /// segment `T` replaced by `__oscen_endpoints_T` (generics stripped).
    /// Its idents carry the requesting statement's span (the wildcard's
    /// `*` token), so resolution errors point at the parent's statement.
    pub manifest_path: syn::Path,
}

/// Parse a `graph!` body and collect its wildcard hoists in declaration
/// order. Returns an empty `Vec` for graphs without wildcards (the caller
/// then compiles exactly as before — zero behavior change).
///
/// Errors accumulate: parse errors, wildcards naming unknown nodes, and
/// wildcards on nodes whose constructor doesn't reveal a type path
/// (`node = make_thing()`) are all reported together.
pub fn scan_wildcards(input: TokenStream) -> Result<Vec<WildcardRequest>, Diagnostics> {
    let mut diags = Diagnostics::new();
    let graph_def = crate::parse::parse_graph_def(input, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    scan_wildcards_parsed(&graph_def)
}

/// [`scan_wildcards`] over an already-parsed body — used by
/// [`expand_graph_entry`], which parses once and reuses the AST for the
/// wildcard-free compile path.
fn scan_wildcards_parsed(graph_def: &GraphDef) -> Result<Vec<WildcardRequest>, Diagnostics> {
    let mut diags = Diagnostics::new();

    // Node name -> declaration, for type-path resolution.
    let node_decls: HashMap<String, &NodeDecl> = graph_def
        .node_decls()
        .map(|n| (n.name.to_string(), n))
        .collect();

    let mut requests = Vec::new();
    for item in &graph_def.items {
        let GraphItem::Input(input) = item else {
            continue;
        };
        let Some(HoistSource {
            node,
            endpoints: HoistEndpoints::Wildcard { span },
        }) = &input.hoist
        else {
            continue;
        };
        let Some(decl) = node_decls.get(&node.to_string()) else {
            diags.push_error(syn::Error::new(
                node.span(),
                format!(
                    "wildcard hoist `input {node}.*;` references unknown node `{node}` \
                     (hoists re-export a declared node's endpoints)"
                ),
            ));
            continue;
        };
        let Some(ty) = &decl.node_type else {
            diags.push_error(syn::Error::new(
                *span,
                format!(
                    "cannot determine the type of node `{node}` for `input {node}.*;`: \
                     wildcard hoists resolve the node type's endpoint manifest from its \
                     constructor path (`Type::new()` or `path::Type::new()`); declare the \
                     node with an explicit type path or hoist endpoints individually"
                ),
            ));
            continue;
        };
        requests.push(WildcardRequest {
            node: node.clone(),
            manifest_path: manifest_path_for(ty, *span),
        });
    }

    if diags.is_empty() {
        Ok(requests)
    } else {
        Err(diags)
    }
}

/// Map a node type path to its manifest macro path: replace the last
/// segment `T` with `__oscen_endpoints_T` and strip its generic arguments
/// (the manifest name is derived from the type name alone). A bare `T`
/// maps to a bare `__oscen_endpoints_T`, which must be in scope at the
/// `graph!` call site — `#[derive(Node)]` re-exports the manifest next to
/// the type, so `use child_crate::*` (or a qualified node path) suffices.
fn manifest_path_for(ty: &syn::Path, span: Span) -> syn::Path {
    let mut path = ty.clone();
    if let Some(last) = path.segments.last_mut() {
        last.ident = Ident::new(
            &format!("__oscen_endpoints_{}", ident_base(&last.ident)),
            span,
        );
        last.arguments = syn::PathArguments::None;
    }
    path
}

/// Emit the endpoint-manifest macro for a node or graph type: an exported
/// `macro_rules!` that invokes a caller-supplied continuation with the
/// type's endpoint list appended to arbitrary passthrough state:
///
/// ```ignore
/// __oscen_endpoints_<TypeName>!($callback:path => ( <passthrough> ));
/// // expands to:
/// $callback! {
///     <passthrough>
///     node_type <TypeName>
///     inputs [ name1: value, name2: stream, name3: event ]
///     outputs [ out1: stream ]
/// }
/// ```
///
/// Each `name: kind` entry may carry an optional parenthesized annotation
/// list — additive metadata consumed by wildcard expansion (entries
/// without annotations parse exactly as before):
///
/// ```ignore
/// inputs [
///     inp: stream (ty = ::oscen::frame::Frame<2>),  // declared frame type
///     cutoff: value (ramp = 256),   // graph!-declared ramp length (frames)
///     level: value (ramped),        // smoothed, length known only at runtime
///     pulse_width: value (priv),    // real endpoint, but not `pub`
/// ]
/// ```
///
/// Annotations may combine (comma-separated, any order). Type tokens in
/// `ty = …` resolve at the *consuming* graph's call site (`macro_rules!`
/// item-path hygiene): `graph!` emitters canonicalize recognized frame
/// types to fully-qualified `::oscen::frame::…` paths, but
/// `#[derive(Node)]` carries the field's literal type tokens, so a bare
/// `Frame<2>` requires the parent to have `Frame` in scope.
///
/// This is the single emitter shared by `#[derive(Node)]` (in
/// `oscen-macros`) and `graph!` codegen; the payload skeleton must stay
/// parseable by [`NodeManifest`]'s `Parse` impl below.
///
/// `#[macro_export]` names are crate-global, so the exported name is
/// mangled `__oscen_endpoints_export_<TypeName>_<hash>`, where `<hash>` is
/// a deterministic FNV-1a digest of the type name plus `definition_tokens`
/// (the derive passes the item's tokens; `graph!` passes the graph body).
/// That keeps two same-named types in different modules of one crate from
/// colliding on the export (only byte-identical same-named definitions
/// still would). Consumers never see the hashed name: the module-local
/// re-export `pub use … as __oscen_endpoints_<TypeName>;` is what
/// `manifest_path_for`-derived paths resolve, it travels with
/// `pub use module::*` chains, and module-local aliases can't collide
/// across modules.
pub fn emit_manifest_export(
    type_name: &Ident,
    inputs: &[ManifestEndpoint],
    outputs: &[ManifestEndpoint],
    definition_tokens: &TokenStream,
) -> TokenStream {
    let hash = export_disambiguator(type_name, definition_tokens);
    let type_base = ident_base(type_name);
    let export_ident = Ident::new(
        &format!("__oscen_endpoints_export_{type_base}_{hash}"),
        Span::call_site(),
    );
    let manifest_ident = Ident::new(&format!("__oscen_endpoints_{type_base}"), Span::call_site());
    let input_entries: Vec<TokenStream> = inputs.iter().map(manifest_entry_tokens).collect();
    let output_entries: Vec<TokenStream> = outputs.iter().map(manifest_entry_tokens).collect();
    quote::quote! {
        #[doc(hidden)]
        #[allow(non_local_definitions)]
        #[macro_export]
        macro_rules! #export_ident {
            ($callback:path => ( $($passthrough:tt)* )) => {
                $callback! {
                    $($passthrough)*
                    node_type #type_name
                    inputs [ #(#input_entries),* ]
                    outputs [ #(#output_entries),* ]
                }
            };
        }
        #[doc(hidden)]
        #[allow(unused_imports)]
        pub use #export_ident as #manifest_ident;
    }
}

/// The bare kind ident (`value` / `stream` / `event` / `asset`) used in
/// manifest endpoint entries — the inverse of `EndpointKind`'s `Parse`.
fn manifest_kind_tokens(kind: EndpointKind) -> TokenStream {
    match kind {
        EndpointKind::Stream => quote::quote! { stream },
        EndpointKind::Value => quote::quote! { value },
        EndpointKind::Event => quote::quote! { event },
        EndpointKind::Asset => quote::quote! { asset },
    }
}

/// One manifest entry: `name: kind` plus the optional parenthesized
/// annotation list (`ty = …`, `ramp = N`, `ramped`, `priv`) — the inverse
/// of [`parse_endpoint_entries`]. Entries without metadata keep the bare
/// `name: kind` form so the grammar stays additive.
fn manifest_entry_tokens(ep: &ManifestEndpoint) -> TokenStream {
    let name = &ep.name;
    let kind = manifest_kind_tokens(ep.kind);
    let mut annotations: Vec<TokenStream> = Vec::new();
    if let Some(ty) = &ep.ty {
        annotations.push(quote::quote! { ty = #ty });
    }
    if ep.generic {
        annotations.push(quote::quote! { generic });
    }
    match ep.ramp {
        ManifestRamp::None => {}
        ManifestRamp::Frames(n) => {
            let lit = proc_macro2::Literal::usize_unsuffixed(n);
            annotations.push(quote::quote! { ramp = #lit });
        }
        ManifestRamp::Declared => annotations.push(quote::quote! { ramped }),
    }
    if ep.private {
        annotations.push(quote::quote! { priv });
    }
    if ep.restricted {
        annotations.push(quote::quote! { restricted });
    }
    if let Some((min, max)) = &ep.range {
        annotations.push(quote::quote! { range = #min .. #max });
    }
    if ep.log {
        annotations.push(quote::quote! { log });
    }
    if let Some(center) = &ep.center {
        annotations.push(quote::quote! { center = #center });
    }
    if let Some(unit) = &ep.unit {
        annotations.push(quote::quote! { unit = #unit });
    }
    if let Some(step) = &ep.step {
        annotations.push(quote::quote! { step = #step });
    }
    if let Some(group) = &ep.group {
        annotations.push(quote::quote! { group = #group });
    }
    if let Some(display) = &ep.display_name {
        annotations.push(quote::quote! { display = #display });
    }
    if annotations.is_empty() {
        quote::quote! { #name: #kind }
    } else {
        quote::quote! { #name: #kind ( #(#annotations),* ) }
    }
}

/// 16-hex-char disambiguator for the crate-global `#[macro_export]` name:
/// FNV-1a (64-bit) over the type name and the definition's token stream.
/// Purely a function of source tokens — deterministic across builds (no
/// time, randomness, or environment).
fn export_disambiguator(type_name: &Ident, definition_tokens: &TokenStream) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    feed(type_name.to_string().as_bytes());
    // Separator so (name, tokens) pairs can't collide by shifting bytes
    // between the two parts (idents never contain NUL).
    feed(&[0]);
    feed(definition_tokens.to_string().as_bytes());
    format!("{hash:016x}")
}

/// Ramp metadata of a manifest endpoint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManifestRamp {
    /// No declared smoothing.
    None,
    /// `ramp = N`: a `graph!`-declared ramp of N frames (`[ramp: N]` on the
    /// child's input spec). Wildcard expansion re-declares the hoisted
    /// parent input with the same `[ramp: N]` spec.
    Frames(usize),
    /// `ramped`: the endpoint is smoothed (a `ValueRampState` field on a
    /// `#[derive(Node)]` type) but the ramp length is a runtime value the
    /// derive cannot see. Wildcard expansion rejects these with a spanned
    /// error telling the user to hoist explicitly with `[ramp: N]`.
    Declared,
}

/// One endpoint entry of a manifest: `name: kind`, plus optional
/// annotations (`ty = …`, `ramp = N`, `ramped`, `priv`, and the param
/// metadata set: `range = a..b`, `log`, `center = e`, `unit = "s"`,
/// `step = e`, `group = "s"`, `display = "s"`).
pub struct ManifestEndpoint {
    pub name: Ident,
    pub kind: EndpointKind,
    /// Declared endpoint type tokens, where known and non-mono (`ty = …`).
    /// Carried for stream endpoints so wildcard hoists preserve frame
    /// types (`Frame<2>`); `None` means mono `f32`, as before.
    pub ty: Option<syn::Type>,
    /// `generic`: the endpoint's declared type mentions one of the node
    /// type's generic parameters (`F`, `T`, ...). The manifest is expanded
    /// before the constructor's type arguments are known, so the carried
    /// `ty` tokens cannot be resolved; wildcard expansion rejects these
    /// with a spanned error instead of emitting unresolved type tokens.
    pub generic: bool,
    /// Declared smoothing (`ramp = N` / `ramped`).
    pub ramp: ManifestRamp,
    /// `priv`: the endpoint exists but its field is not `pub`. Wildcard
    /// expansion skips it (a parent graph writes child fields directly;
    /// privacy applies) — the marker distinguishes present-but-private
    /// from absent.
    pub private: bool,
    /// `restricted`: the field is `pub(crate)`/`pub(super)`/`pub(in …)`.
    /// Wildcard expansion INCLUDES it — the common case is a same-crate
    /// parent graph, which may legally write the field; a cross-scope
    /// hoist fails with rustc's own field-privacy error at the hoist line.
    pub restricted: bool,
    /// Param-spec metadata declared on a `graph!` child's value input,
    /// carried so wildcard hoists through nested graphs keep range/curve/
    /// display metadata instead of silently stripping it. Range/center/
    /// step are raw expression tokens resolved at the parent's call site
    /// (same hygiene caveat as `ty = …`).
    pub range: Option<(syn::Expr, syn::Expr)>,
    pub log: bool,
    pub center: Option<syn::Expr>,
    pub unit: Option<String>,
    pub step: Option<syn::Expr>,
    pub group: Option<String>,
    pub display_name: Option<String>,
}

impl ManifestEndpoint {
    /// An annotation-free entry (`name: kind`).
    pub fn new(name: Ident, kind: EndpointKind) -> Self {
        ManifestEndpoint {
            name,
            kind,
            ty: None,
            generic: false,
            ramp: ManifestRamp::None,
            private: false,
            restricted: false,
            range: None,
            log: false,
            center: None,
            unit: None,
            step: None,
            group: None,
            display_name: None,
        }
    }

    /// Reconstruct the child's `ParamSpec` from the carried metadata, for
    /// re-declaring a wildcard-hoisted input on the parent. `None` when the
    /// entry carries no spec-relevant metadata at all.
    pub fn to_param_spec(&self) -> Option<crate::ast::ParamSpec> {
        let ramp = match self.ramp {
            ManifestRamp::Frames(frames) => Some(frames),
            ManifestRamp::None | ManifestRamp::Declared => None,
        };
        let spec = crate::ast::ParamSpec {
            range: self
                .range
                .clone()
                .map(|(min, max)| crate::ast::RangeSpec { min, max }),
            curve: self.log.then_some(crate::ast::Curve::Logarithmic),
            ramp,
            center: self.center.clone(),
            unit: self.unit.clone(),
            smoother: None,
            step: self.step.clone(),
            display_name: self.display_name.clone(),
            group: self.group.clone(),
        };
        let empty = spec.range.is_none()
            && spec.curve.is_none()
            && spec.ramp.is_none()
            && spec.center.is_none()
            && spec.unit.is_none()
            && spec.step.is_none()
            && spec.display_name.is_none()
            && spec.group.is_none();
        (!empty).then_some(spec)
    }
}

/// The parsed payload of one endpoint-manifest invocation:
///
/// ```ignore
/// node_type <TypeName>
/// inputs [ name1: value, name2: stream (ty = Frame<2>), name3: event ]
/// outputs [ out1: stream ]
/// ```
pub struct NodeManifest {
    pub node_type: Ident,
    pub inputs: Vec<ManifestEndpoint>,
    pub outputs: Vec<ManifestEndpoint>,
}

mod manifest_kw {
    syn::custom_keyword!(node_type);
    syn::custom_keyword!(inputs);
    syn::custom_keyword!(outputs);
}

fn parse_endpoint_entries(input: ParseStream) -> syn::Result<Vec<ManifestEndpoint>> {
    let content;
    bracketed!(content in input);
    let mut entries = Vec::new();
    while !content.is_empty() {
        let name: Ident = content.parse()?;
        content.parse::<Token![:]>()?;
        let kind: EndpointKind = content.parse()?;
        let mut entry = ManifestEndpoint::new(name, kind);
        if content.peek(syn::token::Paren) {
            let annotations;
            syn::parenthesized!(annotations in content);
            while !annotations.is_empty() {
                if annotations.peek(Token![priv]) {
                    annotations.parse::<Token![priv]>()?;
                    entry.private = true;
                } else {
                    let key: Ident = annotations.parse()?;
                    match key.to_string().as_str() {
                        "ty" => {
                            annotations.parse::<Token![=]>()?;
                            entry.ty = Some(annotations.parse()?);
                        }
                        "ramp" => {
                            annotations.parse::<Token![=]>()?;
                            let lit: syn::LitInt = annotations.parse()?;
                            entry.ramp = ManifestRamp::Frames(lit.base10_parse()?);
                        }
                        "ramped" => {
                            entry.ramp = ManifestRamp::Declared;
                        }
                        "generic" => {
                            entry.generic = true;
                        }
                        "restricted" => {
                            entry.restricted = true;
                        }
                        "range" => {
                            annotations.parse::<Token![=]>()?;
                            let expr: syn::Expr = annotations.parse()?;
                            let syn::Expr::Range(range) = expr else {
                                return Err(syn::Error::new(
                                    key.span(),
                                    "`range` annotation expects `min..max`",
                                ));
                            };
                            let (Some(min), Some(max)) = (range.start, range.end) else {
                                return Err(syn::Error::new(
                                    key.span(),
                                    "`range` annotation expects both bounds (`min..max`)",
                                ));
                            };
                            entry.range = Some((*min, *max));
                        }
                        "log" => {
                            entry.log = true;
                        }
                        "center" => {
                            annotations.parse::<Token![=]>()?;
                            entry.center = Some(annotations.parse()?);
                        }
                        "unit" => {
                            annotations.parse::<Token![=]>()?;
                            let lit: syn::LitStr = annotations.parse()?;
                            entry.unit = Some(lit.value());
                        }
                        "step" => {
                            annotations.parse::<Token![=]>()?;
                            entry.step = Some(annotations.parse()?);
                        }
                        "group" => {
                            annotations.parse::<Token![=]>()?;
                            let lit: syn::LitStr = annotations.parse()?;
                            entry.group = Some(lit.value());
                        }
                        "display" => {
                            annotations.parse::<Token![=]>()?;
                            let lit: syn::LitStr = annotations.parse()?;
                            entry.display_name = Some(lit.value());
                        }
                        other => {
                            return Err(syn::Error::new(
                                key.span(),
                                format!(
                                    "unknown endpoint annotation `{other}` in endpoint \
                                     manifest (expected `ty = <Type>`, `generic`, \
                                     `ramp = <frames>`, `ramped`, `priv`, `restricted`, \
                                     `range = <min>..<max>`, `log`, `center = <expr>`, \
                                     `unit = <str>`, `step = <expr>`, `group = <str>`, \
                                     or `display = <str>`)"
                                ),
                            ))
                        }
                    }
                }
                if annotations.peek(Token![,]) {
                    annotations.parse::<Token![,]>()?;
                }
            }
        }
        entries.push(entry);
        if content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
        }
    }
    Ok(entries)
}

impl Parse for NodeManifest {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<manifest_kw::node_type>()?;
        let node_type: Ident = input.parse()?;
        input.parse::<manifest_kw::inputs>()?;
        let inputs = parse_endpoint_entries(input)?;
        input.parse::<manifest_kw::outputs>()?;
        let outputs = parse_endpoint_entries(input)?;
        Ok(NodeManifest {
            node_type,
            inputs,
            outputs,
        })
    }
}

/// Substitute resolved manifests into every wildcard hoist, expanding each
/// into explicit single-endpoint hoists (the existing Phase-2 machinery)
/// in manifest declaration order. Runs before lowering.
///
/// Expansion rules (see docs/ERGONOMICS_PLAN.md §1):
/// - Only the node's **input** endpoints are hoisted (all kinds: value,
///   stream, event). `asset` inputs are skipped — assets bind through
///   `external` declarations, not graph inputs.
/// - Endpoints that are already connection destinations for that node
///   anywhere in the graph are skipped (they're wired manually).
/// - Endpoints already hoisted explicitly (`input voices.cutoff;`, list
///   hoists) are skipped.
/// - A name collision with any other declaration (input, output, node) is
///   an accumulated error at the wildcard's span — rename the colliding
///   declaration or hoist the endpoint explicitly with a rename.
///
/// All generated idents are re-spanned to the wildcard's `*` token so
/// downstream diagnostics point at the parent's statement, never at
/// child-crate manifest tokens.
pub(crate) fn expand_wildcards(
    graph_def: &mut GraphDef,
    manifests: &HashMap<String, NodeManifest>,
    diags: &mut Diagnostics,
) {
    let has_wildcards = graph_def.items.iter().any(is_wildcard_item);
    if !has_wildcards {
        return;
    }

    // (node, endpoint) pairs that are connection destinations anywhere in
    // the graph.
    let mut connected: HashSet<(String, String)> = HashSet::new();
    let mut record_dest = |dest: &ConnectionExpr| {
        if let ConnectionExpr::Field(root, endpoint) = dest {
            // Unwrap indexed roots (`voices[0].frequency`) too: an indexed
            // write still makes broadcast-hoisting that endpoint ambiguous.
            let mut inner: &ConnectionExpr = root;
            while let ConnectionExpr::ArrayIndex(next, _) = inner {
                inner = next;
            }
            if let ConnectionExpr::Ident(node) = inner {
                connected.insert((ident_base(node), ident_base(endpoint)));
            }
        }
    };
    for item in &graph_def.items {
        match item {
            GraphItem::Connection(stmt) => record_dest(&stmt.dest),
            GraphItem::ConnectionBlock(block) => {
                for stmt in &block.0 {
                    record_dest(&stmt.dest);
                }
            }
            _ => {}
        }
    }

    // (node, endpoint) pairs hoisted explicitly (single or list).
    let mut explicit_hoists: HashSet<(String, String)> = HashSet::new();
    for item in &graph_def.items {
        let GraphItem::Input(input) = item else {
            continue;
        };
        match &input.hoist {
            Some(HoistSource {
                node,
                endpoints: HoistEndpoints::Single(ep),
            }) => {
                explicit_hoists.insert((ident_base(node), ident_base(ep)));
            }
            Some(HoistSource {
                node,
                endpoints: HoistEndpoints::List { endpoints, .. },
            }) => {
                for ep in endpoints {
                    explicit_hoists.insert((ident_base(node), ident_base(ep)));
                }
            }
            _ => {}
        }
    }
    // Every non-wildcard declared name (inputs, outputs, nodes, externals):
    // the namespace wildcard expansions must not collide with.
    let mut taken_names = graph_def.declared_names();

    let items = std::mem::take(&mut graph_def.items);
    let mut expanded: Vec<GraphItem> = Vec::with_capacity(items.len());
    for item in items {
        if !is_wildcard_item(&item) {
            expanded.push(item);
            continue;
        }
        let GraphItem::Input(input) = item else {
            unreachable!("is_wildcard_item only matches inputs");
        };
        let Some(HoistSource {
            node,
            endpoints: HoistEndpoints::Wildcard { span },
        }) = &input.hoist
        else {
            unreachable!("is_wildcard_item only matches wildcard hoists");
        };
        let node_key = node.to_string();
        // Membership tests against the r#-stripped sets above.
        let node_base = ident_base(node);
        let Some(manifest) = manifests.get(&node_key) else {
            diags.push_error(syn::Error::new(
                *span,
                format!(
                    "no endpoint manifest resolved for `input {node}.*;` \
                     (wildcard hoists require the node type's \
                     `__oscen_endpoints_<Type>!` manifest, emitted by \
                     `#[derive(Node)]` and `graph!`; make sure the manifest \
                     is reachable from the node's constructor path)"
                ),
            ));
            continue;
        };
        for ep in &manifest.inputs {
            if ep.kind == EndpointKind::Asset {
                // Assets bind through `external` declarations, never as
                // graph inputs.
                continue;
            }
            if ep.private {
                // A hoist writes the child's field directly; privacy
                // forbids that for non-pub fields. Skip, same as when
                // private endpoints were absent from manifests entirely.
                // `restricted` (pub(crate)/pub(super)) endpoints are NOT
                // skipped: the common case is a same-crate parent, and a
                // cross-scope hoist gets rustc's field-privacy error at
                // the hoist line rather than silent parameter loss.
                continue;
            }
            let ep_key = ident_base(&ep.name);
            if connected.contains(&(node_base.clone(), ep_key.clone())) {
                continue;
            }
            if explicit_hoists.contains(&(node_base.clone(), ep_key.clone())) {
                continue;
            }
            if ep.ramp == ManifestRamp::Declared {
                // The child endpoint is smoothed, but the ramp length is a
                // runtime value the manifest cannot carry. A plain hoisted
                // input would silently strip the smoothing; make the user
                // pick a parent ramp explicitly (the wildcard then skips
                // the explicitly hoisted endpoint).
                diags.push_error(syn::Error::new(
                    *span,
                    format!(
                        "wildcard hoist `input {node}.*;` cannot hoist `{ep_key}`: the \
                         child endpoint declares ramp smoothing whose length is only \
                         known at runtime, and a plain hoisted input would silently \
                         drop it; hoist it explicitly with a parent ramp \
                         (`input {node}.{ep_key} [ramp: N];`) — the wildcard then \
                         skips the explicitly hoisted endpoint",
                    ),
                ));
                continue;
            }
            if ep.generic {
                // The endpoint's declared type mentions a generic parameter
                // of the node type; the manifest cannot see the
                // constructor's type arguments, so the carried type tokens
                // are unresolvable here. Expanding anyway emits unresolved
                // type names (or degrades a stream to f32) deep in
                // generated code.
                diags.push_error(syn::Error::new(
                    *span,
                    format!(
                        "wildcard hoist `input {node}.*;` cannot hoist `{ep_key}`: its \
                         declared type is generic on the node type, and manifests \
                         cannot carry the constructor's type arguments; wire it with \
                         an explicit connection (`some_input -> {node}.{ep_key};`) — \
                         the wildcard skips endpoints that are connection destinations",
                    ),
                ));
                continue;
            }
            if !taken_names.insert(ep_key.clone()) {
                diags.push_error(syn::Error::new(
                    *span,
                    format!(
                        "wildcard hoist `input {node}.*;` expands endpoint `{ep_key}`, \
                         which collides with an existing declaration named `{ep_key}`; \
                         rename the declaration, or hoist the endpoint explicitly with \
                         a rename (`input {node}.{ep_key} other_name;`)",
                    ),
                ));
                continue;
            }
            // Re-declare the hoisted input with the child's full param
            // spec: a `ramp = N` becomes the parent's own `[ramp: N]`
            // ValueRampState (the existing explicit hoist-with-ramp path),
            // and range/curve/unit/center/step/group/display metadata
            // carried by the manifest survives into the parent's param
            // registry instead of being silently stripped.
            let spec = ep.to_param_spec();
            // Re-span to the wildcard statement: errors about this input
            // must point at the parent's `input node.*;`, not at the
            // child crate's manifest tokens. Clone + set_span instead of
            // `Ident::new(&ep_key, ..)`: a raw-ident endpoint stringifies
            // as `r#loop`, which Ident::new rejects with a panic.
            let name = {
                let mut name = ep.name.clone();
                name.set_span(*span);
                name
            };
            expanded.push(GraphItem::Input(InputDecl {
                kind: ep.kind,
                name: name.clone(),
                // Carry the child's declared endpoint type (e.g. a stream's
                // `Frame<2>`) so the hoisted parent input matches; `None`
                // stays mono `f32`, as before.
                ty: ep.ty.clone(),
                default: None,
                spec,
                hoist: Some(HoistSource {
                    node: {
                        let mut node_ident = node.clone();
                        node_ident.set_span(*span);
                        node_ident
                    },
                    endpoints: HoistEndpoints::Single(name),
                }),
            }));
        }
    }
    graph_def.items = expanded;
}

// ---------------------------------------------------------------------------
// Two-stage expansion driver (continuation-passing style)
// ---------------------------------------------------------------------------

/// Entry point for the `graph!` proc macro.
///
/// - No wildcard hoists → compiles exactly as before (zero behavior
///   change).
/// - Wildcards present → emits ONE invocation of the first wildcard's
///   manifest macro with `resume_path` (the `__oscen_graph_resume` proc
///   macro) as the continuation, carrying the original graph tokens and
///   the remaining pending wildcards as passthrough state. Each manifest
///   appends its endpoint payload; [`resume`] chains the next manifest or,
///   when all are resolved, re-enters [`crate::compile_with_manifests`].
pub fn expand_graph_entry(
    input: TokenStream,
    resume_path: &syn::Path,
) -> Result<TokenStream, Diagnostics> {
    let mut diags = Diagnostics::new();
    let graph_def = crate::parse::parse_graph_def(input.clone(), &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    let requests = scan_wildcards_parsed(&graph_def)?;
    if requests.is_empty() {
        // No manifests needed: reuse the parsed AST instead of re-parsing.
        // `input` (the original body tokens) still feeds codegen's
        // manifest-export hash.
        return crate::compile_parsed(graph_def, input, &HashMap::new());
    }
    let (first, rest) = requests.split_first().expect("non-empty checked");
    Ok(emit_manifest_invocation(
        resume_path,
        TokenStream::new(),
        rest,
        first,
        &input,
    ))
}

/// Emit one manifest invocation:
///
/// ```ignore
/// path::__oscen_endpoints_T!(<resume_path> => (
///     resolved { <already-resolved `name { manifest }` entries> }
///     pending [ name2 = path2::__oscen_endpoints_T2, ... ]
///     current <node_name>
///     graph { <original graph! tokens> }
/// ));
/// ```
///
/// The manifest macro appends `node_type … inputs […] outputs […]` after
/// the passthrough state; `resume` attributes that payload to `current`.
fn emit_manifest_invocation(
    resume_path: &syn::Path,
    resolved: TokenStream,
    pending: &[WildcardRequest],
    current: &WildcardRequest,
    graph_tokens: &TokenStream,
) -> TokenStream {
    let manifest_path = &current.manifest_path;
    let node = &current.node;
    let pending_entries: Vec<TokenStream> = pending
        .iter()
        .map(|req| {
            let name = &req.node;
            let path = &req.manifest_path;
            quote::quote! { #name = #path }
        })
        .collect();
    quote::quote! {
        #manifest_path!(#resume_path => (
            resolved { #resolved }
            pending [ #(#pending_entries),* ]
            current #node
            graph { #graph_tokens }
        ));
    }
}

/// The passthrough state received by the resume proc macro: the state
/// tokens emitted by [`emit_manifest_invocation`] with one manifest
/// payload appended by the child's manifest macro.
struct ResumeState {
    /// `name { manifest tokens }` entries resolved so far.
    resolved: Vec<(Ident, TokenStream)>,
    /// Wildcards still awaiting their manifest.
    pending: Vec<WildcardRequest>,
    /// The node whose manifest payload trails this invocation.
    current: Ident,
    /// The original `graph!` body.
    graph: TokenStream,
    /// The manifest payload appended by the child's manifest macro.
    manifest: TokenStream,
}

mod resume_kw {
    syn::custom_keyword!(resolved);
    syn::custom_keyword!(pending);
    syn::custom_keyword!(current);
    syn::custom_keyword!(graph);
}

impl Parse for ResumeState {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<resume_kw::resolved>()?;
        let resolved_content;
        syn::braced!(resolved_content in input);
        let mut resolved = Vec::new();
        while !resolved_content.is_empty() {
            let name: Ident = resolved_content.parse()?;
            let manifest_content;
            syn::braced!(manifest_content in resolved_content);
            resolved.push((name, manifest_content.parse::<TokenStream>()?));
        }

        input.parse::<resume_kw::pending>()?;
        let pending_content;
        bracketed!(pending_content in input);
        let mut pending = Vec::new();
        while !pending_content.is_empty() {
            let name: Ident = pending_content.parse()?;
            pending_content.parse::<Token![=]>()?;
            let path: syn::Path = pending_content.parse()?;
            pending.push(WildcardRequest {
                node: name,
                manifest_path: path,
            });
            if pending_content.peek(Token![,]) {
                pending_content.parse::<Token![,]>()?;
            }
        }

        input.parse::<resume_kw::current>()?;
        let current: Ident = input.parse()?;

        input.parse::<resume_kw::graph>()?;
        let graph_content;
        syn::braced!(graph_content in input);
        let graph = graph_content.parse::<TokenStream>()?;

        let manifest = input.parse::<TokenStream>()?;

        Ok(ResumeState {
            resolved,
            pending,
            current,
            graph,
            manifest,
        })
    }
}

/// Continuation for the manifest chain (`__oscen_graph_resume` proc
/// macro): moves the appended manifest payload into the resolved set,
/// then either chains the next pending wildcard's manifest macro or —
/// when all wildcards are resolved — parses the collected manifests and
/// re-enters [`crate::compile_with_manifests`].
pub fn resume(input: TokenStream, resume_path: &syn::Path) -> Result<TokenStream, Diagnostics> {
    let state: ResumeState = syn::parse2(input).map_err(|e| {
        Diagnostics::from(syn::Error::new(
            e.span(),
            format!(
                "malformed wildcard-hoist resume state: {e} \
                 (this macro is internal plumbing for `input node.*;` — \
                 it is not meant to be invoked directly)"
            ),
        ))
    })?;

    let mut resolved = state.resolved;
    resolved.push((state.current, state.manifest));

    if let Some((next, rest)) = state.pending.split_first() {
        let resolved_tokens: TokenStream = resolved
            .iter()
            .map(|(name, manifest)| quote::quote! { #name { #manifest } })
            .collect();
        return Ok(emit_manifest_invocation(
            resume_path,
            resolved_tokens,
            rest,
            next,
            &state.graph,
        ));
    }

    // All manifests collected: parse them and finish compilation.
    let mut diags = Diagnostics::new();
    let mut manifests: HashMap<String, NodeManifest> = HashMap::new();
    for (name, tokens) in resolved {
        match syn::parse2::<NodeManifest>(tokens) {
            Ok(manifest) => {
                manifests.insert(name.to_string(), manifest);
            }
            Err(e) => diags.push_error(syn::Error::new(
                name.span(),
                format!("malformed endpoint manifest for node `{name}`: {e}"),
            )),
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    crate::compile_with_manifests(state.graph, &manifests)
}

fn is_wildcard_item(item: &GraphItem) -> bool {
    matches!(
        item,
        GraphItem::Input(InputDecl {
            hoist: Some(HoistSource {
                endpoints: HoistEndpoints::Wildcard { .. },
                ..
            }),
            ..
        })
    )
}
