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
    pub manifest_path: syn::Path,
    /// Span of the `*` token in the wildcard statement. All wildcard
    /// diagnostics and generated idents use this span so errors point at
    /// the parent's `input node.*;` statement.
    pub span: Span,
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

    // Node name -> declaration, for type-path resolution.
    let mut node_decls: HashMap<String, &NodeDecl> = HashMap::new();
    for item in &graph_def.items {
        match item {
            GraphItem::Node(n) => {
                node_decls.insert(n.name.to_string(), n);
            }
            GraphItem::NodeBlock(b) => {
                for n in &b.0 {
                    node_decls.insert(n.name.to_string(), n);
                }
            }
            _ => {}
        }
    }

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
            span: *span,
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
        last.ident = Ident::new(&format!("__oscen_endpoints_{}", last.ident), span);
        last.arguments = syn::PathArguments::None;
    }
    path
}

/// One endpoint entry of a manifest: `name: kind`.
pub struct ManifestEndpoint {
    pub name: Ident,
    pub kind: EndpointKind,
}

/// The parsed payload of one endpoint-manifest invocation:
///
/// ```ignore
/// node_type <TypeName>
/// inputs [ name1: value, name2: stream, name3: event ]
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
        entries.push(ManifestEndpoint { name, kind });
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
                connected.insert((node.to_string(), endpoint.to_string()));
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
    // Every non-wildcard declared name (inputs, outputs, nodes): the
    // namespace wildcard expansions must not collide with.
    let mut taken_names: HashSet<String> = HashSet::new();
    for item in &graph_def.items {
        match item {
            GraphItem::Input(input) => match &input.hoist {
                Some(HoistSource {
                    node,
                    endpoints: HoistEndpoints::Single(ep),
                }) => {
                    explicit_hoists.insert((node.to_string(), ep.to_string()));
                    taken_names.insert(input.name.to_string());
                }
                Some(HoistSource {
                    node,
                    endpoints: HoistEndpoints::List { endpoints, rename },
                }) => {
                    for ep in endpoints {
                        explicit_hoists.insert((node.to_string(), ep.to_string()));
                        let name = match rename {
                            Some(pat) => pat.apply(ep).to_string(),
                            None => ep.to_string(),
                        };
                        taken_names.insert(name);
                    }
                }
                Some(HoistSource {
                    endpoints: HoistEndpoints::Wildcard { .. },
                    ..
                }) => {}
                None => {
                    taken_names.insert(input.name.to_string());
                }
            },
            GraphItem::Output(output) => {
                taken_names.insert(output.name.to_string());
            }
            GraphItem::Node(n) => {
                taken_names.insert(n.name.to_string());
            }
            GraphItem::NodeBlock(b) => {
                for n in &b.0 {
                    taken_names.insert(n.name.to_string());
                }
            }
            GraphItem::External(e) => {
                taken_names.insert(e.name.to_string());
            }
            _ => {}
        }
    }

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
            let ep_key = ep.name.to_string();
            if connected.contains(&(node_key.clone(), ep_key.clone())) {
                continue;
            }
            if explicit_hoists.contains(&(node_key.clone(), ep_key.clone())) {
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
            // Re-span to the wildcard statement: errors about this input
            // must point at the parent's `input node.*;`, not at the
            // child crate's manifest tokens.
            let name = Ident::new(&ep_key, *span);
            expanded.push(GraphItem::Input(InputDecl {
                kind: ep.kind,
                name: name.clone(),
                ty: None,
                default: None,
                spec: None,
                hoist: Some(HoistSource {
                    node: Ident::new(&node_key, *span),
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
    let requests = scan_wildcards(input.clone())?;
    if requests.is_empty() {
        return crate::compile(input);
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
            let span = name.span();
            pending.push(WildcardRequest {
                node: name,
                manifest_path: path,
                span,
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
