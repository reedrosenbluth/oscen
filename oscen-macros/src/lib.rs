use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod oversample_variants_macro;

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();
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

                if last_segment_ident(&field_ty).as_deref() == Some("SampleRate") {
                    sample_rate_fields.push(field_name.clone());
                }

                let mut input_type_kind = None;
                let mut output_type_kind = None;

                for attr in field.attrs.iter() {
                    if attr.path().is_ident("input") {
                        input_type_kind =
                            Some(parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value));
                    } else if attr.path().is_ident("output") {
                        output_type_kind =
                            Some(parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value));
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

                if let Some(kind) = input_type_kind {
                    // Track event inputs for handle_events and process_event_inputs
                    if kind == EndpointTypeAttr::Event {
                        signal_processor_event_inputs.push((field_name.clone(), input_idx));
                    }

                    input_idents.push(field_name.clone());
                    input_idx += 1;
                }

                if let Some(output_kind) = output_type_kind {
                    // Track event output fields for clear_event_outputs() generation
                    if output_kind == EndpointTypeAttr::Event {
                        let is_array = matches!(&field_ty, syn::Type::Array(_));
                        node_event_output_fields.push((field_name.clone(), is_array));
                    }

                    output_idents.push(field_name.clone());
                    _output_idx += 1;
                }

                // Emit one marker type + EndpointAt impl per endpoint that has a known kind.
                // A field is classified as either an input or an output (the existing walk
                // enforces this by checking input then output) — so taking input first, then
                // output, picks the field's actual endpoint kind.
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
                let #temp_var: ::arrayvec::ArrayVec<_, 32> =
                    self.#field_name.iter().cloned().collect();
                self.#handle_method(&#temp_var);
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

    let expanded = quote! {
        #(#endpoint_errors)*

        #(#endpoint_at_emissions)*

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

fn parse_endpoint_attr(attr: &syn::Attribute) -> Option<EndpointTypeAttr> {
    attr.parse_args::<EndpointTypeAttr>().ok()
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
    match oscen_graph_compiler::compile(input.into()) {
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
