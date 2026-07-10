use oscen_graph_compiler::ast::EndpointKind;
use oscen_graph_compiler::manifest::{ManifestEndpoint, ManifestRamp};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod oversample_variants_macro;

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    // Keep the item's raw tokens: they're hashed into the endpoint
    // manifest's `#[macro_export]` name so same-named node types with
    // different definitions don't collide on the crate-global export.
    let manifest_source = proc_macro2::TokenStream::from(input.clone());
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    // Generic parameter names, for marking manifest entries whose field
    // type can't be resolved without the constructor's type arguments.
    let generic_param_names: std::collections::HashSet<String> = generics
        .type_params()
        .map(|p| p.ident.to_string())
        .chain(generics.const_params().map(|p| p.ident.to_string()))
        .collect();

    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();

    // Manifest entries in declaration order, for the endpoint manifest
    // macro (`__oscen_endpoints_<TypeName>!`). See `emit_manifest_export`.
    let mut manifest_inputs: Vec<ManifestEndpoint> = Vec::new();
    let mut manifest_outputs: Vec<ManifestEndpoint> = Vec::new();
    let mut sample_rate_fields: Vec<syn::Ident> = Vec::new();

    // Errors for removed wrapper endpoint types, emitted alongside the
    // generated impls so the user sees one targeted diagnostic per field
    // instead of a cascade of resolution failures.
    let mut endpoint_errors: Vec<proc_macro2::TokenStream> = Vec::new();

    // Per-endpoint marker types and EndpointAt impls emitted alongside the inherent impl block.
    let mut endpoint_at_emissions: Vec<proc_macro2::TokenStream> = Vec::new();

    // Per-endpoint inherent-assoc-type aliases. Accumulated into one inherent
    // impl block at the end so the marker types are reachable as
    // `<NodeType>::field__Ep` from anywhere `NodeType` is in scope.
    let mut endpoint_assoc_alias_emissions: Vec<proc_macro2::TokenStream> = Vec::new();

    // Track event output fields on the node struct for clear_event_outputs() generation
    let mut node_event_output_fields: Vec<(syn::Ident, bool)> = Vec::new(); // (field_name, is_array)

    // Track event input fields for handle_events and process_event_inputs
    let mut signal_processor_event_inputs = Vec::new(); // (field_name, index)

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut _output_idx: usize = 0;

            for field in fields.named {
                let field_name = field.ident.unwrap();
                let field_ty = field.ty.clone();
                let field_vis = FieldVis::of(&field.vis);

                if last_segment_ident(&field_ty).as_deref() == Some("SampleRate") {
                    sample_rate_fields.push(field_name.clone());
                }

                let mut input_type_kind = None;
                let mut output_type_kind = None;

                for attr in field.attrs.iter() {
                    if attr.path().is_ident("input") {
                        match parse_endpoint_attr(attr) {
                            Ok(kind) => input_type_kind = Some(kind),
                            Err(err) => endpoint_errors.push(err.to_compile_error()),
                        }
                    } else if attr.path().is_ident("output") {
                        match parse_endpoint_attr(attr) {
                            Ok(kind) => output_type_kind = Some(kind),
                            Err(err) => endpoint_errors.push(err.to_compile_error()),
                        }
                    }
                }

                // Event endpoints are still classified by type (EventInput /
                // EventOutput carry real queue machinery). The removed
                // stream/value wrappers get a targeted migration error.
                if input_type_kind.is_none() {
                    match detect_input_kind_from_type(&field_ty) {
                        Some(EndpointTypeAttr::Event) => {
                            input_type_kind = Some(EndpointTypeAttr::Event);
                        }
                        Some(kind) => {
                            let attr_name = match kind {
                                EndpointTypeAttr::Stream => "#[input(stream)]",
                                _ => "#[input(value)]",
                            };
                            endpoint_errors.push(
                                syn::Error::new_spanned(
                                    &field_ty,
                                    format!(
                                        "wrapper endpoint types were removed; declare this \
                                         endpoint as `{attr_name} pub {field_name}: f32`"
                                    ),
                                )
                                .to_compile_error(),
                            );
                        }
                        None => {}
                    }
                }

                if output_type_kind.is_none() {
                    match detect_output_kind_from_type(&field_ty) {
                        Some(EndpointTypeAttr::Event) => {
                            output_type_kind = Some(EndpointTypeAttr::Event);
                        }
                        Some(kind) => {
                            let attr_name = match kind {
                                EndpointTypeAttr::Stream => "#[output(stream)]",
                                _ => "#[output(value)]",
                            };
                            endpoint_errors.push(
                                syn::Error::new_spanned(
                                    &field_ty,
                                    format!(
                                        "wrapper endpoint types were removed; declare this \
                                         endpoint as `{attr_name} pub {field_name}: f32`"
                                    ),
                                )
                                .to_compile_error(),
                            );
                        }
                        None => {}
                    }
                }

                // A field cannot serve as both an input and an output endpoint;
                // silently picking one would misroute (or drop) signals.
                if input_type_kind.is_some() && output_type_kind.is_some() {
                    endpoint_errors.push(
                        syn::Error::new_spanned(
                            &field_name,
                            "a field cannot be both #[input] and #[output]",
                        )
                        .to_compile_error(),
                    );
                    continue;
                }

                if let Some(kind) = input_type_kind {
                    // Track event inputs for handle_events and process_event_inputs
                    if kind == EndpointTypeAttr::Event {
                        signal_processor_event_inputs.push((field_name.clone(), input_idx));
                    }

                    input_idents.push(field_name.clone());
                    // Every endpoint joins the manifest — non-pub fields are
                    // real endpoints, marked `priv` (skipped by wildcard
                    // hoists) or `restricted` (`pub(crate)`/`pub(super)`,
                    // hoistable from the visibility scope) so consumers can
                    // distinguish visibility from absence.
                    manifest_inputs.push(manifest_entry(
                        &field_name,
                        kind,
                        &field_ty,
                        field_vis,
                        &generic_param_names,
                    ));
                    input_idx += 1;
                }

                if let Some(output_kind) = output_type_kind {
                    // Track event output fields for clear_event_outputs() generation
                    if output_kind == EndpointTypeAttr::Event {
                        let is_array = matches!(&field_ty, syn::Type::Array(_));
                        node_event_output_fields.push((field_name.clone(), is_array));
                    }

                    output_idents.push(field_name.clone());
                    manifest_outputs.push(manifest_entry(
                        &field_name,
                        output_kind,
                        &field_ty,
                        field_vis,
                        &generic_param_names,
                    ));
                    _output_idx += 1;
                }

                // Emit one marker type + EndpointAt impl per endpoint that has a known kind.
                // A field is classified as either an input or an output (the conflict check
                // above rejects fields with both), so at most one of the two is Some here.
                let primary_kind = input_type_kind.or(output_type_kind);
                if let Some(kind) = primary_kind {
                    let marker_ident = format_ident!("{}__{}__Ep", name, field_name);
                    let kind_marker = kind_marker_for_attr(kind, &field_ty);
                    let frame_ty = endpoint_frame_type(kind, &field_ty);
                    let assoc_ident = format_ident!("{}__Ep", field_name);
                    endpoint_at_emissions.push(quote! {
                        #[allow(non_camel_case_types)]
                        pub struct #marker_ident;
                        impl #impl_generics ::oscen::dispatch::EndpointAt<#marker_ident>
                            for #name #ty_generics #where_clause
                        {
                            type Kind = #kind_marker;
                            type Frame = #frame_ty;
                        }
                    });
                    endpoint_assoc_alias_emissions.push(quote! {
                        pub type #assoc_ident = #marker_ident;
                    });
                }
            }
        }
    }

    // Generate the inherent `set_sample_rate` method (filled when the struct has
    // a `SampleRate` field, a no-op otherwise so graph codegen can call it
    // uniformly). More than one `SampleRate` field is an error.
    let (set_sample_rate_method, sample_rate_error) = if sample_rate_fields.len() == 1 {
        let field = &sample_rate_fields[0];
        (
            quote! {
                #[inline]
                #[allow(dead_code)]
                pub fn set_sample_rate(&mut self, sample_rate: f32) {
                    self.#field.set(sample_rate);
                }
            },
            quote! {},
        )
    } else if sample_rate_fields.len() > 1 {
        (
            quote! {
                #[inline]
                #[allow(dead_code)]
                pub fn set_sample_rate(&mut self, _sample_rate: f32) {}
            },
            quote! {
                compile_error!("a `#[derive(Node)]` struct may declare at most one `SampleRate` field");
            },
        )
    } else {
        (
            quote! {
                #[inline]
                #[allow(dead_code)]
                pub fn set_sample_rate(&mut self, _sample_rate: f32) {}
            },
            quote! {},
        )
    };

    // Generate handle_events method for static graphs
    let handle_events_method = if !signal_processor_event_inputs.is_empty() {
        let mut event_handler_calls = Vec::new();

        for (field_name, _idx) in &signal_processor_event_inputs {
            let handler_method = format_ident!("on_{}", field_name);
            let handle_method = format_ident!("handle_{}_events", field_name);

            event_handler_calls.push(quote! {
                /// Handle events for this endpoint (called by static graphs)
                #[inline]
                #[allow(dead_code)]
                pub fn #handle_method(
                    &mut self,
                    events: &[::oscen::graph::EventInstance],
                ) {
                    for event in events {
                        self.#handler_method(event);
                    }
                }
            });
        }

        quote! {
            #(#event_handler_calls)*
        }
    } else {
        quote! {}
    };

    // Generate clear_event_outputs() method for static graphs
    let clear_event_outputs_method = if !node_event_output_fields.is_empty() {
        let mut clear_stmts = Vec::new();
        for (field_name, is_array) in &node_event_output_fields {
            if *is_array {
                clear_stmts.push(quote! {
                    for output in &mut self.#field_name {
                        output.clear();
                    }
                });
            } else {
                clear_stmts.push(quote! {
                    self.#field_name.clear();
                });
            }
        }
        quote! {
            /// Clear all event outputs before handlers run.
            /// Called by static graphs at the start of each processing frame.
            #[inline]
            pub fn clear_event_outputs(&mut self) {
                #(#clear_stmts)*
            }
        }
    } else {
        quote! {
            /// Clear all event outputs (no-op for nodes without event outputs).
            #[inline]
            pub fn clear_event_outputs(&mut self) {}
        }
    };

    // Generate process_event_inputs() method for static graphs
    let process_event_inputs_method = if !signal_processor_event_inputs.is_empty() {
        let mut handler_calls = Vec::new();
        for (field_name, _idx) in &signal_processor_event_inputs {
            let handle_method = format_ident!("handle_{}_events", field_name);
            let temp_var = format_ident!("temp_{}_events", field_name);
            handler_calls.push(quote! {
                // Empty-queue fast path: this runs every frame for every
                // event input, and most frames carry no events. The handler
                // iterates the events, so skipping it on empty is a no-op.
                if !self.#field_name.is_empty() {
                    let #temp_var: ::arrayvec::ArrayVec<
                        _,
                        { ::oscen::graph::MAX_STATIC_EVENTS_PER_ENDPOINT },
                    > = self.#field_name.iter().cloned().collect();
                    // Drain the queue so events delivered outside a graph
                    // connection (which would overwrite it) are not replayed
                    // on the next frame.
                    self.#field_name.clear();
                    self.#handle_method(&#temp_var);
                }
            });
        }
        quote! {
            /// Process all event inputs: clear outputs, then dispatch events to handlers.
            /// Called by static graphs before process() - enables uniform codegen without type inference.
            #[inline]
            pub fn process_event_inputs(&mut self) {
                self.clear_event_outputs();
                #(#handler_calls)*
            }
        }
    } else {
        quote! {
            /// Process all event inputs (no-op for nodes without event inputs).
            /// Called by static graphs before process() - enables uniform codegen without type inference.
            #[inline]
            pub fn process_event_inputs(&mut self) {
                self.clear_event_outputs();
            }
        }
    };

    // Endpoint manifest macro: an exported macro_rules "manifest" carrying
    // the node's endpoint list, invoked in continuation-passing style by a
    // parent `graph!` that needs this type's endpoints at expansion time
    // (wildcard hoists: `input voices.*;`). The `#[macro_export]` name is
    // mangled (`__oscen_endpoints_export_<TypeName>_<hash>`, hashing the
    // item's tokens) so the pretty `__oscen_endpoints_<TypeName>` re-export
    // next to the type never collides with the crate-root export, and two
    // same-named node types in different modules of one crate don't collide
    // on the crate-global export either; the re-export travels with
    // `pub use module::*` chains so qualified manifest paths mirror the
    // type's path.
    //
    // NOTE: two byte-identical same-named node type definitions in one
    // crate still collide on the exported macro name (documented
    // limitation; see docs/COOKBOOK.md).
    let manifest = oscen_graph_compiler::manifest::emit_manifest_export(
        &name,
        &manifest_inputs,
        &manifest_outputs,
        &manifest_source,
    );

    let expanded = quote! {
        #(#endpoint_errors)*

        #(#endpoint_at_emissions)*

        #manifest

        #sample_rate_error

        #[allow(non_camel_case_types, dead_code)]
        impl #impl_generics #name #ty_generics #where_clause {
            #(#endpoint_assoc_alias_emissions)*
        }

        impl #impl_generics #name #ty_generics #where_clause {
            #handle_events_method

            #clear_event_outputs_method

            #process_event_inputs_method

            #set_sample_rate_method

            #[allow(dead_code)]
            fn __oscen_suppress_unused(&self) {
                #(let _ = &self.#input_idents;)*
                #(let _ = &self.#output_idents;)*
            }
        }
    };

    TokenStream::from(expanded)
}

/// Build one manifest entry for a `#[derive(Node)]` endpoint field,
/// carrying the metadata the manifest grammar supports:
///
/// - `ty = <field type>` for stream endpoints whose type isn't literally
///   `f32` (so wildcard hoists preserve frame types like `Frame<2>`), and
///   likewise for value endpoints carrying a typed payload (any type other
///   than `f32`/`ValueRampState`, e.g. a `ValuePayload` enum). The derive
///   cannot rewrite arbitrary user types to fully-qualified paths, so the
///   literal tokens are carried — they resolve at the consuming graph's
///   call site, which must have the type name in scope (documented
///   limitation).
/// - `ramped` for value inputs stored as `ValueRampState` (the ramp length
///   is a runtime value the derive cannot see).
/// - `priv` for non-pub fields.
/// Endpoint-field visibility, as far as the manifest cares: `pub` hoists
/// anywhere, `pub(crate)`/`pub(super)`/`pub(in …)` hoists from within the
/// visibility scope (rustc rejects a cross-scope hoist with its own
/// field-privacy error), private never hoists. The restriction level is
/// collapsed to one marker — the consuming graph's crate identity isn't
/// knowable at expansion time, so finer granularity would be unusable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldVis {
    Public,
    Restricted,
    Private,
}

impl FieldVis {
    fn of(vis: &syn::Visibility) -> Self {
        match vis {
            syn::Visibility::Public(_) => FieldVis::Public,
            syn::Visibility::Restricted(_) => FieldVis::Restricted,
            syn::Visibility::Inherited => FieldVis::Private,
        }
    }
}

/// True when `ty`'s tokens mention any of the node struct's generic
/// parameters (`F`, `T`, `const N`, ...) — such a type cannot be carried
/// usefully in the manifest (the constructor's type arguments are unknown
/// at expansion time), so the entry is marked `generic` and wildcard
/// expansion rejects hoisting it.
fn ty_mentions_generic_param(ty: &syn::Type, params: &std::collections::HashSet<String>) -> bool {
    fn walk(ts: proc_macro2::TokenStream, params: &std::collections::HashSet<String>) -> bool {
        ts.into_iter().any(|tt| match tt {
            proc_macro2::TokenTree::Ident(i) => params.contains(&i.to_string()),
            proc_macro2::TokenTree::Group(g) => walk(g.stream(), params),
            _ => false,
        })
    }
    !params.is_empty() && walk(quote!(#ty), params)
}

fn manifest_entry(
    field_name: &syn::Ident,
    kind: EndpointTypeAttr,
    field_ty: &syn::Type,
    field_vis: FieldVis,
    generic_params: &std::collections::HashSet<String>,
) -> ManifestEndpoint {
    let manifest_kind = match kind {
        EndpointTypeAttr::Stream => EndpointKind::Stream,
        EndpointTypeAttr::Value => EndpointKind::Value,
        EndpointTypeAttr::Event => EndpointKind::Event,
        EndpointTypeAttr::Asset => EndpointKind::Asset,
    };
    let mut entry = ManifestEndpoint::new(field_name.clone(), manifest_kind);
    entry.private = field_vis == FieldVis::Private;
    entry.restricted = field_vis == FieldVis::Restricted;
    match kind {
        EndpointTypeAttr::Stream => {
            if quote!(#field_ty).to_string() != "f32" {
                entry.ty = Some(field_ty.clone());
            }
        }
        EndpointTypeAttr::Value => {
            if last_segment_ident(field_ty).as_deref() == Some("ValueRampState") {
                entry.ramp = ManifestRamp::Declared;
            } else if quote!(#field_ty).to_string() != "f32" {
                // Typed value payload (a `ValuePayload` type such as an
                // enum or bool): carry the field's literal type tokens,
                // same hygiene caveat as the stream branch above.
                entry.ty = Some(field_ty.clone());
            }
        }
        EndpointTypeAttr::Event | EndpointTypeAttr::Asset => {}
    }
    if entry.ty.is_some() && ty_mentions_generic_param(field_ty, generic_params) {
        entry.generic = true;
    }
    entry
}

fn parse_endpoint_attr(attr: &syn::Attribute) -> syn::Result<EndpointTypeAttr> {
    match &attr.meta {
        // Bare `#[input]` / `#[output]` defaults to a value endpoint.
        syn::Meta::Path(_) => Ok(EndpointTypeAttr::Value),
        // Anything with arguments must parse; unknown kinds (e.g. a typo'd
        // `#[input(strem)]`) are compile errors instead of silently
        // defaulting to a value endpoint.
        _ => attr.parse_args::<EndpointTypeAttr>(),
    }
}

fn kind_marker_for_attr(kind: EndpointTypeAttr, ty: &syn::Type) -> proc_macro2::TokenStream {
    // Array-of-events maps to EventArrayKind; otherwise the scalar kind.
    if matches!(kind, EndpointTypeAttr::Event) {
        if let syn::Type::Array(_) = ty {
            return quote! { ::oscen::dispatch::EventArrayKind };
        }
    }
    match kind {
        EndpointTypeAttr::Stream => quote! { ::oscen::dispatch::StreamKind },
        EndpointTypeAttr::Value => quote! { ::oscen::dispatch::ValueKind },
        EndpointTypeAttr::Event => quote! { ::oscen::dispatch::EventKind },
        EndpointTypeAttr::Asset => quote! { ::oscen::dispatch::AssetKind },
    }
}

fn detect_input_kind_from_type(ty: &syn::Type) -> Option<EndpointTypeAttr> {
    match last_segment_ident(ty)?.as_str() {
        "StreamInput" => Some(EndpointTypeAttr::Stream),
        "ValueInput" => Some(EndpointTypeAttr::Value),
        "EventInput" => Some(EndpointTypeAttr::Event),
        _ => None,
    }
}

fn detect_output_kind_from_type(ty: &syn::Type) -> Option<EndpointTypeAttr> {
    match last_segment_ident(ty)?.as_str() {
        "StreamOutput" => Some(EndpointTypeAttr::Stream),
        "ValueOutput" => Some(EndpointTypeAttr::Value),
        "EventOutput" => Some(EndpointTypeAttr::Event),
        _ => None,
    }
}

/// Determine the `EndpointAt::Frame` associated type from an endpoint field's
/// declared type. Stream endpoints use the field type itself (`f32`,
/// `Frame<N>`), with endpoint arrays using their element's frame type.
/// Value and event endpoints don't carry an audio frame — their payloads may
/// be arbitrary types (e.g. `OscilloscopeHandle`) — so they map to `f32`,
/// which is what cross-rate value kernels operate on.
fn endpoint_frame_type(kind: EndpointTypeAttr, ty: &syn::Type) -> proc_macro2::TokenStream {
    if !matches!(kind, EndpointTypeAttr::Stream) {
        return quote! { f32 };
    }
    match ty {
        syn::Type::Array(arr) => endpoint_frame_type(kind, &arr.elem),
        syn::Type::Path(_) => quote! { #ty },
        _ => quote! { f32 },
    }
}

fn last_segment_ident(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string())
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EndpointTypeAttr {
    Stream,
    Value,
    Event,
    /// A runtime-bound immutable audio asset input (`#[input(asset)]`). Maps to
    /// the `AssetKind` dispatch marker; never resampled (no `CrossRateKernel`).
    Asset,
}

impl syn::parse::Parse for EndpointTypeAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(EndpointTypeAttr::Value);
        }

        let ident: syn::Ident = input.parse()?;
        match ident.to_string().as_str() {
            "stream" => Ok(EndpointTypeAttr::Stream),
            "value" => Ok(EndpointTypeAttr::Value),
            "event" => Ok(EndpointTypeAttr::Event),
            "asset" => Ok(EndpointTypeAttr::Asset),
            other => Err(syn::Error::new(
                ident.span(),
                format!("unknown endpoint type `{}`", other),
            )),
        }
    }
}

/// Declarative macro for defining audio processing graphs.
///
/// # Example
/// ```ignore
/// graph! {
///     input value cutoff = 3000.0 [20.0..20000.0, log, ramp(1323)];
///     input event gate;
///     output stream out;
///
///     nodes {
///         osc = PolyBlepOscillator::saw(440.0, 0.6);
///         filter = TptFilter::new(3000.0, 0.707);
///     }
///
///     connection {
///         cutoff -> filter.cutoff();
///         osc.output() -> filter.input();
///         filter.output() -> out;
///     }
/// }
/// ```
#[proc_macro]
pub fn graph(input: TokenStream) -> TokenStream {
    // Two-stage expansion: graphs with wildcard hoists (`input node.*;`)
    // expand to a chain of endpoint-manifest macro invocations with
    // `__oscen_graph_resume` as the continuation; graphs without compile
    // directly (zero behavior change).
    match oscen_graph_compiler::manifest::expand_graph_entry(input.into(), &resume_path()) {
        Ok(ts) => ts.into(),
        Err(diags) => diags.into_compile_errors().into(),
    }
}

/// Path of the resume continuation as seen from downstream crates.
/// `oscen-lib` re-exports the proc macro next to `graph`, so
/// `::oscen::__oscen_graph_resume` resolves everywhere `::oscen::graph`
/// does.
fn resume_path() -> syn::Path {
    syn::parse_quote!(::oscen::__oscen_graph_resume)
}

/// Internal continuation for wildcard hoists (`input node.*;`) — not part
/// of the public API.
///
/// A `graph!` with wildcard hoists expands to an invocation of the first
/// wildcard node type's endpoint-manifest macro with this proc macro as
/// the continuation; the manifest appends the node's endpoint list to the
/// passthrough state. This macro then chains the next pending manifest
/// or, when all wildcards are resolved, resumes normal graph compilation
/// with the collected endpoint sets.
#[doc(hidden)]
#[proc_macro]
pub fn __oscen_graph_resume(input: TokenStream) -> TokenStream {
    match oscen_graph_compiler::manifest::resume(input.into(), &resume_path()) {
        Ok(ts) => ts.into(),
        Err(diags) => diags.into_compile_errors().into(),
    }
}

/// Materialize multiple `graph!` variants from a single body, substituting an
/// integer factor for each occurrence of the placeholder `{FACTOR}`.
///
/// # Example
/// ```ignore
/// oversample_variants! {
///     base_name: MyGraph;
///     factors: [1, 2, 4];
///     body: {
///         output stream audio_out;
///         nodes {
///             osc = PolyBlepOscillator::saw(440.0, 0.6) * {FACTOR};
///         }
///         connections {
///             [sinc] osc.output -> audio_out;
///         }
///     }
/// }
/// ```
///
/// This produces graph types `MyGraph_1x`, `MyGraph_2x`, `MyGraph_4x`.
#[proc_macro]
pub fn oversample_variants(input: TokenStream) -> TokenStream {
    oversample_variants_macro::oversample_variants_impl(input)
}
