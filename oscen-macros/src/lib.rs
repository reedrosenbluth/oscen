use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(Node, attributes(input, output))]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let endpoints_name = format_ident!("{}Endpoints", name);

    let mut input_fields = Vec::new();
    let mut output_fields = Vec::new();
    let mut input_names = Vec::new();
    let mut output_names = Vec::new();
    let mut input_indices = Vec::new();
    let mut output_indices = Vec::new();
    let mut input_getters = Vec::new();

    // Extract field information
    if let Data::Struct(data_struct) = input.data {
        if let Fields::Named(fields) = data_struct.fields {
            let mut input_idx: usize = 0;
            let mut output_idx: usize = 0;

            for field in fields.named {
                let field_name = field.ident.unwrap();
                let has_input = field.attrs.iter().any(|attr| attr.path().is_ident("input"));
                let has_output = field
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("output"));

                if has_input {
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

                    input_names.push(field_name.to_string());
                    input_indices.push(input_idx);
                    input_idx += 1;
                }

                if has_output {
                    output_fields.push(quote! {
                        pub fn #field_name(&self) -> OutputEndpoint {
                            OutputEndpoint::new(self.outputs[#output_idx])
                        }
                    });
                    output_names.push(field_name.to_string());
                    output_indices.push(output_idx);
                    output_idx += 1;
                }
            }
        }
    }

    let expanded = quote! {
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
        }

        impl ProcessingNode for #name {
            type Endpoints = #endpoints_name;

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

        impl EndpointDefinition for #name {
            fn input_endpoints(&self) -> &'static [EndpointMetadata] {
                const INPUTS: &[EndpointMetadata] = &[
                    #(EndpointMetadata { name: #input_names, index: #input_indices },)*
                ];
                INPUTS
            }

            fn output_endpoints(&self) -> &'static [EndpointMetadata] {
                const OUTPUTS: &[EndpointMetadata] = &[
                    #(EndpointMetadata { name: #output_names, index: #output_indices },)*
                ];
                OUTPUTS
            }
        }
    };

    TokenStream::from(expanded)
}
