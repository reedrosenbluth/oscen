use super::ast::*;
use super::type_check::TypeContext;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

pub fn generate(graph_def: &GraphDef) -> Result<TokenStream> {
    let mut ctx = CodegenContext::new();

    // Collect all declarations
    for item in &graph_def.items {
        ctx.collect_item(item)?;
    }

    // Validate connections
    ctx.validate_connections()?;

    // Generate either module-level struct or expression-level builder
    if let Some(name) = &graph_def.name {
        ctx.generate_module_struct(name)
    } else {
        ctx.generate_closure()
    }
}

struct CodegenContext {
    inputs: Vec<InputDecl>,
    outputs: Vec<OutputDecl>,
    nodes: Vec<NodeDecl>,
    connections: Vec<ConnectionStmt>,
}

impl CodegenContext {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
        }
    }

    /// Construct the Endpoints type from a node type
    /// E.g., PolyBlepOscillator -> PolyBlepOscillatorEndpoints
    fn construct_endpoints_type(node_type: &syn::Path) -> TokenStream {
        // Build a new path with "Endpoints" appended to the last segment
        let segments: Vec<_> = node_type.segments.iter().collect();

        if segments.is_empty() {
            return quote! { UnknownEndpoints };
        }

        // Get all segments except the last
        let leading_segments = &segments[..segments.len() - 1];

        // Get the last segment and create the Endpoints version
        let last_segment = segments.last().unwrap();
        let type_name = &last_segment.ident;
        let endpoints_ident = syn::Ident::new(
            &format!("{}Endpoints", type_name),
            type_name.span()
        );

        if leading_segments.is_empty() {
            // Simple type like PolyBlepOscillator
            quote! { #endpoints_ident }
        } else {
            // Qualified type like oscen::PolyBlepOscillator
            quote! { #(#leading_segments)::* :: #endpoints_ident }
        }
    }

    fn collect_item(&mut self, item: &GraphItem) -> Result<()> {
        match item {
            GraphItem::Input(input) => {
                self.inputs.push(input.clone());
            }
            GraphItem::Output(output) => {
                self.outputs.push(output.clone());
            }
            GraphItem::Node(node) => {
                self.nodes.push(node.clone());
            }
            GraphItem::NodeBlock(block) => {
                self.nodes.extend_from_slice(&block.0);
            }
            GraphItem::Connection(conn) => {
                self.connections.push(conn.clone());
            }
            GraphItem::ConnectionBlock(block) => {
                self.connections.extend_from_slice(&block.0);
            }
        }
        Ok(())
    }

    fn validate_connections(&self) -> Result<()> {
        let mut type_ctx = TypeContext::new();

        // Register all inputs and outputs
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }

        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }

        // Validate each connection
        for conn in &self.connections {
            // Validate source and destination independently
            type_ctx.validate_source(&conn.source)?;
            type_ctx.validate_destination(&conn.dest)?;

            // Validate type compatibility
            type_ctx.validate_connection(&conn.source, &conn.dest)?;
        }

        Ok(())
    }

    fn generate_context_struct(&self) -> TokenStream {
        let mut fields = vec![
            quote! { pub graph: ::oscen::Graph }
        ];

        // Add input fields
        for input in &self.inputs {
            let name = &input.name;
            let ty = match input.kind {
                EndpointKind::Value => quote! { ::oscen::ValueParam },
                EndpointKind::Event => quote! { ::oscen::EventParam },
                EndpointKind::Stream => quote! { ::oscen::StreamInput },
            };
            fields.push(quote! { pub #name: #ty });
        }

        // Add output fields
        for output in &self.outputs {
            let name = &output.name;
            let ty = match output.kind {
                EndpointKind::Value => quote! { ::oscen::ValueParam },
                EndpointKind::Event => quote! { ::oscen::EventParam },
                EndpointKind::Stream => quote! { ::oscen::StreamOutput },
            };
            fields.push(quote! { pub #name: #ty });
        }

        // Add node handle fields
        for node in &self.nodes {
            let name = &node.name;
            if let Some(node_type) = &node.node_type {
                // Construct the Endpoints type by appending "Endpoints" to the node type
                // E.g., PolyBlepOscillator -> PolyBlepOscillatorEndpoints
                let endpoints_type = Self::construct_endpoints_type(node_type);
                if let Some(array_size) = node.array_size {
                    // Array of endpoints
                    fields.push(quote! { pub #name: [#endpoints_type; #array_size] });
                } else {
                    // Single endpoint
                    fields.push(quote! { pub #name: #endpoints_type });
                }
            }
        }

        quote! {
            #[allow(dead_code)]
            pub struct GraphContext {
                #(#fields),*
            }
        }
    }

    fn generate_context_impl(&self) -> Result<TokenStream> {
        let input_params = self.generate_input_params();
        let node_creation = self.generate_node_creation();
        let connections = self.generate_connections()?;
        let output_assignments = self.generate_output_assignments();
        let struct_init = self.generate_struct_init();

        Ok(quote! {
            impl GraphContext {
                #[allow(unused_variables, unused_mut)]
                pub fn new(sample_rate: f32) -> Self {
                    let mut graph = ::oscen::Graph::new(sample_rate);

                    // Create input parameters
                    #(#input_params)*

                    // Create nodes
                    #(#node_creation)*

                    // Make connections
                    #(#connections)*

                    // Assign outputs
                    #(#output_assignments)*

                    Self {
                        graph,
                        #struct_init
                    }
                }
            }
        })
    }

    fn generate_input_params(&self) -> Vec<TokenStream> {
        self.inputs.iter().map(|input| {
            let name = &input.name;
            let default_val = input.default.as_ref();

            match input.kind {
                EndpointKind::Value => {
                    if let Some(default) = default_val {
                        quote! {
                            let #name = graph.value_param(#default);
                        }
                    } else {
                        quote! {
                            let #name = graph.value_param(0.0);
                        }
                    }
                }
                EndpointKind::Event => {
                    // For event inputs, create an EventParam (which uses EventPassthrough internally)
                    quote! {
                        let #name = graph.event_param();
                    }
                }
                EndpointKind::Stream => {
                    quote! {
                        let #name = {
                            let key = graph.allocate_endpoint(::oscen::graph::EndpointType::Stream);
                            ::oscen::StreamInput::new(::oscen::graph::InputEndpoint::new(key))
                        };
                    }
                }
            }
        }).collect()
    }

    fn generate_node_creation(&self) -> Vec<TokenStream> {
        self.nodes.iter().flat_map(|node| {
            let name = &node.name;
            let constructor = &node.constructor;

            if let Some(array_size) = node.array_size {
                // Generate multiple instances with indexed names
                (0..array_size).map(|i| {
                    let indexed_name = syn::Ident::new(
                        &format!("{}_{}", name, i),
                        name.span()
                    );
                    quote! {
                        let #indexed_name = graph.add_node(#constructor);
                    }
                }).collect::<Vec<_>>()
            } else {
                // Single instance
                vec![quote! {
                    let #name = graph.add_node(#constructor);
                }]
            }
        }).collect()
    }

    fn generate_connections(&self) -> Result<Vec<TokenStream>> {
        if self.connections.is_empty() {
            return Ok(vec![]);
        }

        let mut regular_connections = Vec::new();
        let mut output_assignments = Vec::new();
        let mut temp_counter = 0;

        for conn in &self.connections {
            // Check if destination is an output
            if let ConnectionExpr::Ident(dest_ident) = &conn.dest {
                if self.outputs.iter().any(|o| o.name == *dest_ident) {
                    // This is an output assignment with potential intermediate values
                    let (stmts, final_expr) = self.generate_expr_with_temps(&conn.source, &mut temp_counter)?;
                    output_assignments.extend(stmts);
                    output_assignments.push(quote! {
                        let #dest_ident = #final_expr;
                    });
                    continue;
                }
            }

            // Regular connection
            let source = self.generate_connection_expr(&conn.source)?;
            let dest = self.generate_connection_expr(&conn.dest)?;
            regular_connections.push(quote! {
                #source >> #dest
            });
        }

        let mut result = Vec::new();

        if !regular_connections.is_empty() {
            result.push(quote! {
                graph.connect_all(vec![
                    #(#regular_connections),*
                ]);
            });
        }

        result.extend(output_assignments);

        Ok(result)
    }

    /// Generate an expression, extracting binary operations to temporary variables
    fn generate_expr_with_temps(&self, expr: &ConnectionExpr, counter: &mut usize) -> Result<(Vec<TokenStream>, TokenStream)> {
        match expr {
            ConnectionExpr::Binary(left, op, right) => {
                // Generate left side (might create temps)
                let (mut stmts, left_expr) = self.generate_expr_with_temps(left, counter)?;

                // Generate right side (might create temps)
                let (right_stmts, right_expr) = self.generate_expr_with_temps(right, counter)?;
                stmts.extend(right_stmts);

                // Create a temp variable for this binary operation
                let temp_name = syn::Ident::new(&format!("__temp_{}", counter), proc_macro2::Span::call_site());
                *counter += 1;

                let operation = match op {
                    BinaryOp::Mul => quote! { graph.multiply(#left_expr, #right_expr) },
                    BinaryOp::Add => quote! { graph.add(#left_expr, #right_expr) },
                    BinaryOp::Sub => quote! { graph.subtract(#left_expr, #right_expr) },
                    BinaryOp::Div => quote! { graph.divide(#left_expr, #right_expr) },
                };

                stmts.push(quote! {
                    let #temp_name = #operation;
                });

                Ok((stmts, quote! { #temp_name }))
            }
            _ => {
                // No binary operations, generate normally
                let expr_code = self.generate_connection_expr(expr)?;
                Ok((vec![], expr_code))
            }
        }
    }

    fn generate_connection_expr(&self, expr: &ConnectionExpr) -> Result<TokenStream> {
        match expr {
            ConnectionExpr::Ident(ident) => {
                Ok(quote! { #ident })
            }
            ConnectionExpr::ArrayIndex(array_expr, index) => {
                // For array[idx], we need to check if the base is an identifier
                // If it is, translate to base_idx (e.g., voices[0] -> voices_0)
                if let ConnectionExpr::Ident(base_name) = &**array_expr {
                    let indexed_name = syn::Ident::new(
                        &format!("{}_{}", base_name, index),
                        base_name.span()
                    );
                    Ok(quote! { #indexed_name })
                } else {
                    // For more complex expressions, use actual array indexing
                    let array = self.generate_connection_expr(array_expr)?;
                    Ok(quote! { #array[#index] })
                }
            }
            ConnectionExpr::Method(obj, method, args) => {
                let obj_expr = self.generate_connection_expr(obj)?;
                if args.is_empty() {
                    Ok(quote! { #obj_expr.#method() })
                } else {
                    Ok(quote! { #obj_expr.#method(#(#args),*) })
                }
            }
            ConnectionExpr::Binary(left, op, right) => {
                let left_expr = self.generate_connection_expr(left)?;
                let right_expr = self.generate_connection_expr(right)?;

                match op {
                    BinaryOp::Mul => {
                        Ok(quote! { graph.multiply(#left_expr, #right_expr) })
                    }
                    BinaryOp::Add => {
                        Ok(quote! { graph.add(#left_expr, #right_expr) })
                    }
                    BinaryOp::Sub => {
                        Ok(quote! { graph.subtract(#left_expr, #right_expr) })
                    }
                    BinaryOp::Div => {
                        Ok(quote! { graph.divide(#left_expr, #right_expr) })
                    }
                }
            }
            ConnectionExpr::Literal(lit) => {
                Ok(quote! { #lit })
            }
            ConnectionExpr::Call(func, args) => {
                let arg_exprs: Result<Vec<_>> = args.iter()
                    .map(|arg| self.generate_connection_expr(arg))
                    .collect();
                let arg_exprs = arg_exprs?;
                Ok(quote! { #func(#(#arg_exprs),*) })
            }
        }
    }

    /// Generate a builder struct with a clean build() method
    fn generate_closure(&self) -> Result<TokenStream> {
        let context_struct = self.generate_context_struct();
        let input_params = self.generate_input_params();
        let node_creation = self.generate_node_creation();
        let connections = self.generate_connections()?;
        let struct_init = self.generate_struct_init();

        Ok(quote! {
            {
                // Generate the struct at module scope so it's visible
                #context_struct

                struct __GraphBuilder;

                impl __GraphBuilder {
                    fn build(self, sample_rate: f32) -> GraphContext {
                        let mut graph = ::oscen::Graph::new(sample_rate);

                        // Create input parameters
                        #(#input_params)*

                        // Create nodes
                        #(#node_creation)*

                        // Make connections
                        #(#connections)*

                        GraphContext {
                            graph,
                            #struct_init
                        }
                    }
                }

                __GraphBuilder
            }
        })
    }

    fn generate_output_assignments(&self) -> Vec<TokenStream> {
        // For now, outputs are handled in connections
        // We might need to track which connection assigns to each output
        vec![]
    }

    fn generate_struct_init(&self) -> TokenStream {
        let mut fields = Vec::new();

        for input in &self.inputs {
            let name = &input.name;
            fields.push(quote! { #name });
        }

        for output in &self.outputs {
            let name = &output.name;
            fields.push(quote! { #name });
        }

        // Add node handles
        for node in &self.nodes {
            let name = &node.name;
            if let Some(array_size) = node.array_size {
                // Generate array initializer: [name_0, name_1, ...]
                let indexed_names: Vec<_> = (0..array_size)
                    .map(|i| {
                        syn::Ident::new(&format!("{}_{}", name, i), name.span())
                    })
                    .collect();
                fields.push(quote! { #name: [#(#indexed_names),*] });
            } else {
                fields.push(quote! { #name });
            }
        }

        quote! { #(#fields),* }
    }

    /// Generate the Endpoints struct for a graph (e.g., VoiceEndpoints)
    fn generate_endpoints_struct(&self, graph_name: &syn::Ident) -> TokenStream {
        let endpoints_name = syn::Ident::new(
            &format!("{}Endpoints", graph_name),
            graph_name.span()
        );

        let mut fields = vec![
            quote! { node_key: ::oscen::NodeKey }
        ];

        let mut accessor_methods = Vec::new();

        // Add input fields and accessor methods
        for input in &self.inputs {
            let field_name = &input.name;
            let method_name = &input.name;
            let (ty, accessor_ty) = match input.kind {
                EndpointKind::Value => (
                    quote! { ::oscen::ValueInput },
                    quote! { ::oscen::ValueInput }
                ),
                EndpointKind::Event => (
                    quote! { ::oscen::EventInput },
                    quote! { ::oscen::EventInput }
                ),
                EndpointKind::Stream => (
                    quote! { ::oscen::StreamInput },
                    quote! { ::oscen::StreamInput }
                ),
            };
            fields.push(quote! { #field_name: #ty });
            accessor_methods.push(quote! {
                pub fn #method_name(&self) -> #accessor_ty {
                    self.#field_name
                }
            });
        }

        // Add output fields and accessor methods
        for output in &self.outputs {
            let field_name = &output.name;
            let method_name = &output.name;
            let (ty, accessor_ty) = match output.kind {
                EndpointKind::Value => (
                    quote! { ::oscen::ValueOutput },
                    quote! { ::oscen::ValueOutput }
                ),
                EndpointKind::Event => (
                    quote! { ::oscen::EventOutput },
                    quote! { ::oscen::EventOutput }
                ),
                EndpointKind::Stream => (
                    quote! { ::oscen::StreamOutput },
                    quote! { ::oscen::StreamOutput }
                ),
            };
            fields.push(quote! { #field_name: #ty });
            accessor_methods.push(quote! {
                pub fn #method_name(&self) -> #accessor_ty {
                    self.#field_name
                }
            });
        }

        quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #endpoints_name {
                #(#fields),*
            }

            impl #endpoints_name {
                #(#accessor_methods)*
            }
        }
    }

    /// Generate SignalProcessor implementation for graph
    fn generate_signal_processor_impl(&self, name: &syn::Ident) -> TokenStream {
        // Generate code to route inputs from context to internal graph
        let mut input_routing = Vec::new();
        for (idx, input) in self.inputs.iter().enumerate() {
            let field_name = &input.name;
            let routing_code = match input.kind {
                EndpointKind::Stream => {
                    quote! {
                        let value = context.stream(#idx);
                        if let Some(state) = self.graph.endpoints.get_mut(self.#field_name.key()) {
                            state.set_scalar(value);
                        }
                    }
                }
                EndpointKind::Value => {
                    quote! {
                        let value = context.value_scalar(#idx);
                        self.graph.set_value(self.#field_name, value);
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        for event in context.events(#idx) {
                            self.graph.queue_event(
                                self.#field_name,
                                event.frame_offset,
                                event.payload.clone()
                            );
                        }
                    }
                }
            };
            input_routing.push(routing_code);
        }

        // Get the first output to return (required by SignalProcessor)
        let return_expr = if let Some(first_output) = self.outputs.first() {
            let field_name = &first_output.name;
            quote! {
                self.graph.get_value(&self.#field_name).unwrap_or(0.0)
            }
        } else {
            quote! { 0.0 }
        };

        // Generate code to emit event outputs
        let mut event_output_routing = Vec::new();
        for (idx, output) in self.outputs.iter().enumerate() {
            if output.kind == EndpointKind::Event {
                let field_name = &output.name;
                event_output_routing.push(quote! {
                    self.graph.drain_events(self.#field_name, |event| {
                        context.emit_event(#idx, event.clone());
                    });
                });
            }
        }

        quote! {
            impl ::oscen::SignalProcessor for #name {
                fn process<'a>(
                    &mut self,
                    _sample_rate: f32,
                    context: &mut ::oscen::ProcessingContext<'a>
                ) -> f32 {
                    // Route external inputs to internal graph endpoints
                    #(#input_routing)*

                    // Process internal graph
                    let _ = self.graph.process();

                    // Route event outputs from internal graph to external context
                    #(#event_output_routing)*

                    // Return primary output
                    #return_expr
                }
            }
        }
    }

    /// Generate ProcessingNode implementation for graph
    fn generate_processing_node_impl(&self, name: &syn::Ident) -> TokenStream {
        let endpoints_name = syn::Ident::new(
            &format!("{}Endpoints", name),
            name.span()
        );

        // Generate ENDPOINT_DESCRIPTORS
        let mut descriptors = Vec::new();
        for input in &self.inputs {
            let input_name = input.name.to_string();
            let endpoint_type = match input.kind {
                EndpointKind::Stream => quote! { ::oscen::graph::EndpointType::Stream },
                EndpointKind::Value => quote! { ::oscen::graph::EndpointType::Value },
                EndpointKind::Event => quote! { ::oscen::graph::EndpointType::Event },
            };
            descriptors.push(quote! {
                ::oscen::graph::EndpointDescriptor::new(
                    #input_name,
                    #endpoint_type,
                    ::oscen::graph::EndpointDirection::Input
                )
            });
        }

        for output in &self.outputs {
            let output_name = output.name.to_string();
            let endpoint_type = match output.kind {
                EndpointKind::Stream => quote! { ::oscen::graph::EndpointType::Stream },
                EndpointKind::Value => quote! { ::oscen::graph::EndpointType::Value },
                EndpointKind::Event => quote! { ::oscen::graph::EndpointType::Event },
            };
            descriptors.push(quote! {
                ::oscen::graph::EndpointDescriptor::new(
                    #output_name,
                    #endpoint_type,
                    ::oscen::graph::EndpointDirection::Output
                )
            });
        }

        // Generate create_endpoints implementation
        let mut endpoint_fields = vec![quote! { node_key }];
        let mut input_assignments = Vec::new();
        let mut output_assignments = Vec::new();

        for (idx, input) in self.inputs.iter().enumerate() {
            let field_name = &input.name;
            let constructor = match input.kind {
                EndpointKind::Stream => quote! {
                    ::oscen::StreamInput::new(::oscen::graph::InputEndpoint::new(inputs[#idx]))
                },
                EndpointKind::Value => quote! {
                    ::oscen::ValueInput::new(::oscen::graph::InputEndpoint::new(inputs[#idx]))
                },
                EndpointKind::Event => quote! {
                    ::oscen::EventInput::new(::oscen::graph::InputEndpoint::new(inputs[#idx]))
                },
            };
            input_assignments.push(quote! {
                let #field_name = #constructor;
            });
            endpoint_fields.push(quote! { #field_name });
        }

        for (idx, output) in self.outputs.iter().enumerate() {
            let field_name = &output.name;
            let constructor = match output.kind {
                EndpointKind::Stream => quote! {
                    ::oscen::StreamOutput::new(outputs[#idx])
                },
                EndpointKind::Value => quote! {
                    ::oscen::ValueOutput::new(outputs[#idx])
                },
                EndpointKind::Event => quote! {
                    ::oscen::EventOutput::new(outputs[#idx])
                },
            };
            output_assignments.push(quote! {
                let #field_name = #constructor;
            });
            endpoint_fields.push(quote! { #field_name });
        }

        quote! {
            impl ::oscen::ProcessingNode for #name {
                type Endpoints = #endpoints_name;

                const ENDPOINT_DESCRIPTORS: &'static [::oscen::graph::EndpointDescriptor] = &[
                    #(#descriptors),*
                ];

                fn create_endpoints(
                    node_key: ::oscen::NodeKey,
                    inputs: arrayvec::ArrayVec<
                        ::oscen::ValueKey,
                        { ::oscen::graph::MAX_NODE_ENDPOINTS }
                    >,
                    outputs: arrayvec::ArrayVec<
                        ::oscen::ValueKey,
                        { ::oscen::graph::MAX_NODE_ENDPOINTS }
                    >,
                ) -> Self::Endpoints {
                    #(#input_assignments)*
                    #(#output_assignments)*

                    #endpoints_name {
                        #(#endpoint_fields),*
                    }
                }
            }
        }
    }

    /// Generate a module-level struct definition with a constructor
    fn generate_module_struct(&self, name: &syn::Ident) -> Result<TokenStream> {
        let mut fields = vec![
            quote! { pub graph: ::oscen::Graph }
        ];

        // Add input fields
        for input in &self.inputs {
            let field_name = &input.name;
            let ty = match input.kind {
                EndpointKind::Value => quote! { ::oscen::ValueParam },
                EndpointKind::Event => quote! { ::oscen::EventParam },
                EndpointKind::Stream => quote! { ::oscen::StreamInput },
            };
            fields.push(quote! { pub #field_name: #ty });
        }

        // Add output fields
        for output in &self.outputs {
            let field_name = &output.name;
            let ty = match output.kind {
                EndpointKind::Value => quote! { ::oscen::ValueParam },
                EndpointKind::Event => quote! { ::oscen::EventParam },
                EndpointKind::Stream => quote! { ::oscen::StreamOutput },
            };
            fields.push(quote! { pub #field_name: #ty });
        }

        // Add node handle fields
        for node in &self.nodes {
            let field_name = &node.name;
            if let Some(node_type) = &node.node_type {
                let endpoints_type = Self::construct_endpoints_type(node_type);
                if let Some(array_size) = node.array_size {
                    // Array of endpoints
                    fields.push(quote! { pub #field_name: [#endpoints_type; #array_size] });
                } else {
                    // Single endpoint
                    fields.push(quote! { pub #field_name: #endpoints_type });
                }
            }
        }

        let input_params = self.generate_input_params();
        let node_creation = self.generate_node_creation();
        let connections = self.generate_connections()?;
        let struct_init = self.generate_struct_init();

        // Generate the additional trait implementations
        let endpoints_struct = self.generate_endpoints_struct(name);
        let signal_processor_impl = self.generate_signal_processor_impl(name);
        let processing_node_impl = self.generate_processing_node_impl(name);

        Ok(quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #name {
                #(#fields),*
            }

            impl #name {
                #[allow(unused_variables, unused_mut)]
                pub fn new(sample_rate: f32) -> Self {
                    let mut graph = ::oscen::Graph::new(sample_rate);

                    // Create input parameters
                    #(#input_params)*

                    // Create nodes
                    #(#node_creation)*

                    // Make connections
                    #(#connections)*

                    Self {
                        graph,
                        #struct_init
                    }
                }
            }

            // Generate Endpoints struct
            #endpoints_struct

            // Generate SignalProcessor implementation
            #signal_processor_impl

            // Generate ProcessingNode implementation
            #processing_node_impl
        })
    }
}

// Add Clone impls for AST types
impl Clone for InputDecl {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.clone(),
            default: self.default.clone(),
            spec: None, // Skip spec for now
        }
    }
}

impl Clone for OutputDecl {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.clone(),
        }
    }
}

impl Clone for NodeDecl {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            constructor: self.constructor.clone(),
            node_type: self.node_type.clone(),
            array_size: self.array_size,
        }
    }
}

impl Clone for ConnectionStmt {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            dest: self.dest.clone(),
        }
    }
}

impl Clone for ConnectionExpr {
    fn clone(&self) -> Self {
        match self {
            Self::Ident(i) => Self::Ident(i.clone()),
            Self::ArrayIndex(expr, idx) => Self::ArrayIndex(expr.clone(), *idx),
            Self::Method(obj, method, args) => {
                Self::Method(obj.clone(), method.clone(), args.clone())
            }
            Self::Binary(left, op, right) => {
                Self::Binary(left.clone(), *op, right.clone())
            }
            Self::Literal(lit) => Self::Literal(lit.clone()),
            Self::Call(func, args) => Self::Call(func.clone(), args.clone()),
        }
    }
}
