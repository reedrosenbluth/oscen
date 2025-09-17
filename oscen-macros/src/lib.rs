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
    let mut input_types = Vec::new();
    let mut output_types = Vec::new();
    let mut input_getters = Vec::new();
    let mut input_idents = Vec::new();
    let mut output_idents = Vec::new();

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut output_idx: usize = 0;

            for field in fields.named {
                let field_name = field.ident.unwrap();
                let mut input_type = None;
                let mut output_type = None;

                for attr in field.attrs.iter() {
                    if attr.path().is_ident("input") {
                        let ty = parse_endpoint_type(attr)
                            .unwrap_or_else(|| quote! { EndpointType::Value });
                        input_type = Some(ty);
                    } else if attr.path().is_ident("output") {
                        let ty = parse_endpoint_type(attr)
                            .unwrap_or_else(|| quote! { EndpointType::Value });
                        output_type = Some(ty);
                    }
                }

                if let Some(endpoint_ty) = input_type {
                    input_fields.push(quote! {
                        pub fn #field_name(&self) -> InputEndpoint {
                            InputEndpoint::new(self.inputs[#input_idx])
                        }
                    });

                    let getter_name = format_ident!("get_{}", field_name);
                    input_getters.push(quote! {
                        pub fn #getter_name(&self, inputs: &[f32]) -> f32 {
                            inputs[#input_idx]
                        }
                    });

                    input_types.push(endpoint_ty.clone());
                    input_idents.push(field_name.clone());
                    input_idx += 1;
                }

                if let Some(endpoint_ty) = output_type {
                    output_fields.push(quote! {
                        pub fn #field_name(&self) -> OutputEndpoint {
                            OutputEndpoint::new(self.outputs[#output_idx])
                        }
                    });
                    output_types.push(endpoint_ty.clone());
                    output_idents.push(field_name.clone());
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
            #(#input_getters)*

            #[allow(dead_code)]
            fn __oscen_suppress_unused(&self) {
                #(let _ = &self.#input_idents;)*
                #(let _ = &self.#output_idents;)*
            }
        }

        impl ProcessingNode for #name {
            type Endpoints = #endpoints_name;

            const INPUT_TYPES: &'static [EndpointType] = &[#(#input_types),*];

            const OUTPUT_TYPES: &'static [EndpointType] = &[#(#output_types),*];

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

fn parse_endpoint_type(attr: &syn::Attribute) -> Option<TokenStream2> {
    attr.parse_args::<EndpointTypeAttr>()
        .ok()
        .map(|attr| match attr {
            EndpointTypeAttr::Stream => quote! { EndpointType::Stream },
            EndpointTypeAttr::Value => quote! { EndpointType::Value },
            EndpointTypeAttr::Event => quote! { EndpointType::Event },
        })
}

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
