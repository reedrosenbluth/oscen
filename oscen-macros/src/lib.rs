use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let endpoints_name = format_ident!("{}Endpoints", name);

    let mut input_fields = Vec::new();
    let mut output_fields = Vec::new();
    let mut input_scalar_getters = Vec::new();
    let mut input_value_ref_getters = Vec::new();
    let mut input_event_getters = Vec::new();
    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();
    let mut endpoint_descriptors = Vec::new();
    let mut output_type_kinds = Vec::new();

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut output_idx: usize = 0;

            for field in fields.named {
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
                    let accessor = match accessor_kind {
                        EndpointTypeAttr::Stream => quote! {
                            pub fn #field_name(&self) -> ::oscen::graph::types::StreamInput {
                                ::oscen::graph::types::StreamInput::new(InputEndpoint::new(self.inputs[#input_idx]))
                            }
                        },
                        EndpointTypeAttr::Event => quote! {
                            pub fn #field_name(&self) -> ::oscen::graph::types::EventInput {
                                ::oscen::graph::types::EventInput::new(InputEndpoint::new(self.inputs[#input_idx]))
                            }
                        },
                        EndpointTypeAttr::Value => quote! {
                            pub fn #field_name(&self) -> ::oscen::graph::types::ValueInput {
                                ::oscen::graph::types::ValueInput::new(InputEndpoint::new(self.inputs[#input_idx]))
                            }
                        },
                    };
                    input_fields.push(accessor);

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
                            output_kind = parse_endpoint_attr(attr).unwrap_or(EndpointTypeAttr::Value);
                        }
                    }
                    output_type_kinds.push(output_kind);

                    let output_type_token = match output_kind {
                        EndpointTypeAttr::Stream => quote! { ::oscen::graph::types::StreamOutput },
                        EndpointTypeAttr::Value => quote! { ::oscen::graph::types::ValueOutput },
                        EndpointTypeAttr::Event => quote! { ::oscen::graph::types::EventOutput },
                    };

                    output_fields.push(quote! {
                        pub fn #field_name(&self) -> #output_type_token {
                            #output_type_token::new(self.outputs[#output_idx])
                        }
                    });
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

    let expanded = quote! {
        #[allow(dead_code)]
        #[derive(Debug)]
        pub struct #endpoints_name {
            node_key: NodeKey,
            inputs: arrayvec::ArrayVec<ValueKey, 16>,
            outputs: arrayvec::ArrayVec<ValueKey, 16>,
        }

        impl #endpoints_name {
            #(#input_fields)*
            #(#output_fields)*

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
                inputs: arrayvec::ArrayVec<ValueKey, 16>,
                outputs: arrayvec::ArrayVec<ValueKey, 16>
            ) -> Self::Endpoints {
                #endpoints_name {
                    node_key,
                    inputs,
                    outputs,
                }
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
