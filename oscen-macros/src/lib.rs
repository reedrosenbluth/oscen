use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod graph_macro;

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let endpoints_name = format_ident!("{}Endpoints", name);

    let mut endpoint_fields = Vec::new(); // Struct field definitions for Endpoints
    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();
    let mut endpoint_descriptors = Vec::new();
    let mut create_endpoints_assignments = Vec::new(); // Field assignments in create_endpoints
    let mut value_input_fields = Vec::new(); // Track (field_name, index) for value inputs

    let mut has_event_fields_in_endpoints = false; // Track if Endpoints struct has event fields

    // Track event output fields on the node struct for clear_event_outputs() generation
    let mut node_event_output_fields: Vec<(syn::Ident, bool)> = Vec::new(); // (field_name, is_array)

    // Track event input fields for handle_events and process_event_inputs
    let mut signal_processor_event_inputs = Vec::new(); // (field_name, index)

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut output_idx: usize = 0;

            for field in fields.named {
                let field_name = field.ident.unwrap();
                let field_name_str = field_name.to_string();
                let field_ty = field.ty.clone();
                let mut input_type: Option<(TokenStream2, EndpointTypeAttr)> = None;
                let mut output_type: Option<(TokenStream2, EndpointTypeAttr)> = None;
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

                if input_type_kind.is_none() {
                    input_type_kind = detect_input_kind_from_type(&field_ty);
                }

                if output_type_kind.is_none() {
                    output_type_kind = detect_output_kind_from_type(&field_ty);
                }

                if let Some(kind) = input_type_kind {
                    let ty = endpoint_type_tokens(kind);
                    input_type = Some((ty, kind));
                }

                if let Some(kind) = output_type_kind {
                    let ty = endpoint_type_tokens(kind);
                    output_type = Some((ty, kind));
                }

                if let Some((endpoint_ty, _kind_tag)) = input_type {
                    let descriptor_ty = endpoint_ty.clone();
                    let accessor_kind = input_type_kind.unwrap_or(EndpointTypeAttr::Value);

                    // Generate field type based on endpoint kind
                    let field_type = match accessor_kind {
                        EndpointTypeAttr::Stream => quote! { ::oscen::graph::types::StreamInput },
                        EndpointTypeAttr::Event => event_input_field_type(&field_ty),
                        EndpointTypeAttr::Value => quote! { ::oscen::graph::types::ValueInput },
                    };

                    // Generate field definition for Endpoints struct
                    endpoint_fields.push(quote! {
                        pub #field_name: #field_type
                    });

                    // Generate field assignment in create_endpoints
                    // Event inputs have built-in storage and take no arguments
                    if accessor_kind == EndpointTypeAttr::Event {
                        has_event_fields_in_endpoints = true;
                        create_endpoints_assignments.push(quote! {
                            #field_name: #field_type::new()
                        });
                        signal_processor_event_inputs.push((field_name.clone(), input_idx));
                    } else {
                        create_endpoints_assignments.push(quote! {
                            #field_name: #field_type::new(InputEndpoint::new(inputs[#input_idx]))
                        });
                    }

                    // Track value inputs for default_values() generation
                    if accessor_kind == EndpointTypeAttr::Value {
                        value_input_fields.push((field_name.clone(), input_idx));
                    }

                    input_idents.push(field_name.clone());
                    endpoint_descriptors.push(quote! {
                        ::oscen::graph::types::EndpointDescriptor::new(
                            #field_name_str,
                            #descriptor_ty,
                            ::oscen::graph::types::EndpointDirection::Input,
                        )
                    });

                    input_idx += 1;
                }

                if let Some((descriptor_ty, output_kind)) = output_type {
                    let output_type_token = match output_kind {
                        EndpointTypeAttr::Stream => quote! { ::oscen::graph::types::StreamOutput },
                        EndpointTypeAttr::Value => quote! { ::oscen::graph::types::ValueOutput },
                        EndpointTypeAttr::Event => event_output_field_type(&field_ty),
                    };

                    // Check if this is an array event output (skip in Endpoints struct)
                    let is_array_event_output = output_kind == EndpointTypeAttr::Event && matches!(&field_ty, syn::Type::Array(_));

                    if !is_array_event_output {
                        // Generate field definition for Endpoints struct
                        endpoint_fields.push(quote! {
                            pub #field_name: #output_type_token
                        });

                        // Generate field assignment in create_endpoints
                        // Event outputs have built-in storage and take no arguments
                        if output_kind == EndpointTypeAttr::Event {
                            has_event_fields_in_endpoints = true;
                            create_endpoints_assignments.push(quote! {
                                #field_name: #output_type_token::new()
                            });
                        } else {
                            create_endpoints_assignments.push(quote! {
                                #field_name: #output_type_token::new(outputs[#output_idx])
                            });
                        }
                    }

                    // Track event output fields for clear_event_outputs() generation
                    if output_kind == EndpointTypeAttr::Event {
                        let is_array = matches!(&field_ty, syn::Type::Array(_));
                        node_event_output_fields.push((field_name.clone(), is_array));
                    }

                    output_idents.push(field_name.clone());
                    endpoint_descriptors.push(quote! {
                        ::oscen::graph::types::EndpointDescriptor::new(
                            #field_name_str,
                            #descriptor_ty,
                            ::oscen::graph::types::EndpointDirection::Output,
                        )
                    });
                    output_idx += 1;
                }
            }
        }
    }

    // Generate default_values entries
    let default_value_entries: Vec<_> = value_input_fields
        .iter()
        .map(|(field_name, idx)| {
            quote! { (#idx, self.#field_name) }
        })
        .collect();

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

    // Generate Endpoints struct with conditional Copy derive
    // Event types have built-in storage (StaticEventQueue) which doesn't implement Copy
    let endpoints_struct = if has_event_fields_in_endpoints {
        quote! {
            #[allow(dead_code)]
            #[derive(Debug, Clone)]
            pub struct #endpoints_name {
                pub node_key: NodeKey,
                #(#endpoint_fields),*
            }
        }
    } else {
        quote! {
            #[allow(dead_code)]
            #[derive(Debug, Copy, Clone)]
            pub struct #endpoints_name {
                pub node_key: NodeKey,
                #(#endpoint_fields),*
            }
        }
    };

    let expanded = quote! {
        // Endpoints struct for typed endpoint handles
        #endpoints_struct

        impl #endpoints_name {
            pub fn node_key(&self) -> NodeKey {
                self.node_key
            }
        }

        impl #impl_generics #name #ty_generics #where_clause {
            #handle_events_method

            #clear_event_outputs_method

            #process_event_inputs_method

            #[allow(dead_code)]
            fn __oscen_suppress_unused(&self) {
                #(let _ = &self.#input_idents;)*
                #(let _ = &self.#output_idents;)*
            }
        }

        impl #impl_generics ProcessingNode for #name #ty_generics #where_clause {
            type Endpoints = #endpoints_name;

            const ENDPOINT_DESCRIPTORS: &'static [::oscen::graph::types::EndpointDescriptor] = &[
                #(#endpoint_descriptors),*
            ];

            fn create_endpoints(
                node_key: NodeKey,
                inputs: arrayvec::ArrayVec<ValueKey, { ::oscen::graph::MAX_NODE_ENDPOINTS }>,
                outputs: arrayvec::ArrayVec<ValueKey, { ::oscen::graph::MAX_NODE_ENDPOINTS }>
            ) -> Self::Endpoints {
                #endpoints_name {
                    node_key,
                    #(#create_endpoints_assignments),*
                }
            }

            fn default_values(&self) -> Vec<(usize, f32)> {
                vec![
                    #(#default_value_entries),*
                ]
            }
        }
    };

    TokenStream::from(expanded)
}

fn parse_endpoint_attr(attr: &syn::Attribute) -> Option<EndpointTypeAttr> {
    attr.parse_args::<EndpointTypeAttr>().ok()
}

fn endpoint_type_tokens(attr: EndpointTypeAttr) -> TokenStream2 {
    match attr {
        EndpointTypeAttr::Stream => quote! { ::oscen::graph::EndpointType::Stream },
        EndpointTypeAttr::Value => quote! { ::oscen::graph::EndpointType::Value },
        EndpointTypeAttr::Event => quote! { ::oscen::graph::EndpointType::Event },
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

fn event_input_field_type(ty: &syn::Type) -> TokenStream2 {
    if last_segment_ident(ty).as_deref() == Some("EventInput") {
        quote! { #ty }
    } else {
        quote! { ::oscen::graph::types::EventInput }
    }
}

fn event_output_field_type(ty: &syn::Type) -> TokenStream2 {
    // Check if it's an array type first
    if let syn::Type::Array(array_ty) = ty {
        // Check if the array element is EventOutput
        if last_segment_ident(&array_ty.elem).as_deref() == Some("EventOutput") {
            // Preserve the full array type
            return quote! { #ty };
        }
    }

    // Otherwise check if it's a direct EventOutput type
    if last_segment_ident(ty).as_deref() == Some("EventOutput") {
        quote! { #ty }
    } else {
        quote! { ::oscen::graph::types::EventOutput }
    }
}

fn last_segment_ident(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        type_path.path.segments.last().map(|seg| seg.ident.to_string())
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
    graph_macro::graph_impl(input)
}
