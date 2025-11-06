use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod graph_macro;

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let endpoints_name = format_ident!("{}Endpoints", name);
    let io_name = format_ident!("{}IO", name);

    let mut endpoint_fields = Vec::new(); // Struct field definitions for Endpoints
    let mut io_fields = Vec::new(); // Struct field definitions for IO
    let mut input_scalar_getters = Vec::new();
    let mut input_value_ref_getters = Vec::new();
    let mut input_event_getters = Vec::new();
    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();
    let mut endpoint_descriptors = Vec::new();
    let mut create_endpoints_assignments = Vec::new(); // Field assignments in create_endpoints
    let mut value_input_fields = Vec::new(); // Track (field_name, index) for value inputs

    // For generating SignalProcessor impl
    let mut stream_input_names = Vec::new(); // Names of stream input fields
    let mut stream_output_names = Vec::new(); // Names of stream output fields
    let mut all_stream_fields_public = true; // Track if all stream fields are pub (opt-in signal)

    // Track event I/O for determining if IO struct needs lifetime parameter
    let mut event_input_idx = 0usize;
    let mut event_output_idx = 0usize;

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut output_idx: usize = 0;

            for field in fields.named {
                let field_vis = field.vis.clone(); // Capture visibility before consuming field
                let field_name = field.ident.unwrap();
                let field_name_str = field_name.to_string();

                let mut input_type: Option<(TokenStream2, EndpointTypeAttr)> = None;
                let mut input_type_kind = None;
                let mut output_type = None;

                for attr in field.attrs.iter() {
                    if attr.path().is_ident("input") {
                        let kind = parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value);
                        let ty = endpoint_type_tokens(kind);
                        input_type = Some((ty, kind));
                        input_type_kind = Some(kind);
                    } else if attr.path().is_ident("output") {
                        let kind = parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value);
                        let ty = endpoint_type_tokens(kind);
                        output_type = Some(ty);
                    }
                }

                if let Some((endpoint_ty, _kind_tag)) = input_type {
                    let descriptor_ty = endpoint_ty.clone();
                    let accessor_kind = input_type_kind.unwrap_or(EndpointTypeAttr::Value);

                    // Generate field type based on endpoint kind
                    let field_type = match accessor_kind {
                        EndpointTypeAttr::Stream => quote! { ::oscen::graph::types::StreamInput },
                        EndpointTypeAttr::Event => quote! { ::oscen::graph::types::EventInput },
                        EndpointTypeAttr::Value => quote! { ::oscen::graph::types::ValueInput },
                    };

                    // Generate field definition for Endpoints struct
                    endpoint_fields.push(quote! {
                        pub #field_name: #field_type
                    });

                    // Generate field assignment in create_endpoints
                    create_endpoints_assignments.push(quote! {
                        #field_name: #field_type::new(InputEndpoint::new(inputs[#input_idx]))
                    });

                    // Add to IO struct if stream or event
                    match accessor_kind {
                        EndpointTypeAttr::Stream => {
                            io_fields.push(quote! {
                                pub #field_name: f32
                            });
                            stream_input_names.push(field_name.clone());
                            // Check if this stream field is public (opt-in for auto SignalProcessor)
                            if !matches!(field_vis, syn::Visibility::Public(_)) {
                                all_stream_fields_public = false;
                            }
                        }
                        EndpointTypeAttr::Event => {
                            io_fields.push(quote! {
                                pub #field_name: &'io [::oscen::graph::EventInstance]
                            });
                            event_input_idx += 1;
                        }
                        EndpointTypeAttr::Value => {
                            // Value inputs stay in State (node struct), not IO
                        }
                    }

                    input_idents.push(field_name.clone());
                    endpoint_descriptors.push(quote! {
                        ::oscen::graph::types::EndpointDescriptor::new(
                            #field_name_str,
                            #descriptor_ty,
                            ::oscen::graph::types::EndpointDirection::Input,
                        )
                    });

                    if let Some(kind) = input_type_kind {
                        let read_name = format_ident!("get_{}", field_name);
                        match kind {
                            EndpointTypeAttr::Stream => {
                                input_scalar_getters.push(quote! {
                                    pub fn #read_name<'a>(&self, context: &::oscen::graph::ProcessingContext<'a>) -> f32 {
                                        context.stream(#input_idx)
                                    }
                                });
                            }
                            EndpointTypeAttr::Value => {
                                input_scalar_getters.push(quote! {
                                    pub fn #read_name<'a>(&self, context: &::oscen::graph::ProcessingContext<'a>) -> f32 {
                                        context.value_scalar(#input_idx)
                                    }
                                });

                                let value_ref_name = format_ident!("value_ref_{}", field_name);
                                input_value_ref_getters.push(quote! {
                                    pub fn #value_ref_name<'a>(&self, context: &::oscen::graph::ProcessingContext<'a>) -> Option<::oscen::graph::ValueRef<'a>> {
                                        context.value(#input_idx)
                                    }
                                });

                                // Track value inputs for default_values() generation
                                value_input_fields.push((field_name.clone(), input_idx));
                            }
                            EndpointTypeAttr::Event => {
                                let events_name = format_ident!("events_{}", field_name);
                                input_event_getters.push(quote! {
                                    pub fn #events_name<'a>(&self, context: &'a ::oscen::graph::ProcessingContext<'a>) -> &'a [::oscen::graph::EventInstance] {
                                        context.events(#input_idx)
                                    }
                                });
                            }
                        }
                    }

                    input_idx += 1;
                }

                if let Some(endpoint_ty) = output_type {
                    let descriptor_ty = endpoint_ty.clone();

                    // Determine output type from endpoint_ty
                    let mut output_kind = EndpointTypeAttr::Value; // default
                    for attr in field.attrs.iter() {
                        if attr.path().is_ident("output") {
                            output_kind =
                                parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value);
                        }
                    }

                    let output_type_token = match output_kind {
                        EndpointTypeAttr::Stream => quote! { ::oscen::graph::types::StreamOutput },
                        EndpointTypeAttr::Value => quote! { ::oscen::graph::types::ValueOutput },
                        EndpointTypeAttr::Event => quote! { ::oscen::graph::types::EventOutput },
                    };

                    // Generate field definition for Endpoints struct
                    endpoint_fields.push(quote! {
                        pub #field_name: #output_type_token
                    });

                    // Generate field assignment in create_endpoints
                    create_endpoints_assignments.push(quote! {
                        #field_name: #output_type_token::new(outputs[#output_idx])
                    });

                    // Add to IO struct if stream or event
                    match output_kind {
                        EndpointTypeAttr::Stream => {
                            io_fields.push(quote! {
                                pub #field_name: f32
                            });
                            stream_output_names.push(field_name.clone());
                            // Check if this stream field is public (opt-in for auto SignalProcessor)
                            if !matches!(field_vis, syn::Visibility::Public(_)) {
                                all_stream_fields_public = false;
                            }
                        }
                        EndpointTypeAttr::Event => {
                            io_fields.push(quote! {
                                pub #field_name: ::std::vec::Vec<::oscen::graph::EventInstance>
                            });
                            event_output_idx += 1;
                        }
                        EndpointTypeAttr::Value => {
                            // Value outputs stay in State (node struct), not IO
                        }
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

    // Generate IO struct with lifetime parameter only if there are event endpoints
    let has_event_endpoints = event_input_idx > 0 || event_output_idx > 0;
    let io_struct = if io_fields.is_empty() {
        // Empty IO struct (no stream/event endpoints)
        quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #io_name {
                _marker: ::std::marker::PhantomData<()>,
            }

            impl #io_name {
                pub fn new() -> Self {
                    Self {
                        _marker: ::std::marker::PhantomData,
                    }
                }
            }

            impl Default for #io_name {
                fn default() -> Self {
                    Self::new()
                }
            }
        }
    } else if has_event_endpoints {
        // IO struct with lifetime parameter for event slices
        quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #io_name<'io> {
                #(#io_fields),*
            }
        }
    } else {
        // IO struct without lifetime parameter (only stream endpoints)
        quote! {
            #[allow(dead_code)]
            #[derive(Debug, Default, Copy, Clone)]
            pub struct #io_name {
                #(#io_fields),*
            }
        }
    };

    let expanded = quote! {
        // IO struct for stream and event endpoints
        #io_struct

        // Endpoints struct for typed endpoint handles
        #[allow(dead_code)]
        #[derive(Debug, Copy, Clone)]
        pub struct #endpoints_name {
            pub node_key: NodeKey,
            #(#endpoint_fields),*
        }

        impl #endpoints_name {
            pub fn node_key(&self) -> NodeKey {
                self.node_key
            }
        }

        impl #name {
            #(#input_scalar_getters)*
            #(#input_value_ref_getters)*
            #(#input_event_getters)*

            #[allow(dead_code)]
            fn __oscen_suppress_unused(&self) {
                #(let _ = &self.#input_idents;)*
                #(let _ = &self.#output_idents;)*
            }
        }

        impl ProcessingNode for #name {
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

    // Generate SignalProcessor implementation ONLY if:
    // 1. There are stream inputs/outputs AND
    // 2. ALL stream fields are pub (opt-in signal for auto-generation) AND
    // 3. There are NO value inputs or event inputs (those need manual implementations)
    let has_stream_fields = !stream_input_names.is_empty() || !stream_output_names.is_empty();
    let has_value_or_event_inputs = !value_input_fields.is_empty() || event_input_idx > 0;
    let signal_processor_impl = if has_stream_fields && all_stream_fields_public && !has_value_or_event_inputs {
        let populate_stream_inputs = stream_input_names.iter().map(|field_name| {
            let getter_name = format_ident!("get_{}", field_name);
            quote! {
                self.#field_name = self.#getter_name(context);
            }
        });

        quote! {
            impl ::oscen::graph::SignalProcessor for #name {
                /// Auto-generated wrapper that populates stream fields from context and calls user's process().
                ///
                /// Users implement: pub fn process(&mut self, sample_rate: f32) -> f32
                fn process<'a>(&mut self, sample_rate: f32, context: &mut ::oscen::graph::ProcessingContext<'a>) -> f32 {
                    // Populate stream input fields directly on self
                    #(#populate_stream_inputs)*

                    // Call user-defined processing logic using fully qualified syntax
                    // This calls the inherent impl's process(), not this trait method
                    #name::process(self, sample_rate)
                }
            }

            impl #name {
                /// Auto-generated wrapper for compile-time graphs.
                ///
                /// Assumes stream fields are already wired externally.
                #[inline]
                pub fn process_internal(&mut self, sample_rate: f32) -> f32 {
                    // Calls user's process() method
                    #name::process(self, sample_rate)
                }
            }
        }
    } else {
        quote! {}  // Don't generate if no stream fields
    };

    let full_expansion = quote! {
        #expanded
        #signal_processor_impl
    };

    TokenStream::from(full_expansion)
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

#[derive(Clone, Copy)]
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
