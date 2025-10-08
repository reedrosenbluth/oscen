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
                fields.push(quote! { pub #name: #endpoints_type });
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
        self.nodes.iter().map(|node| {
            let name = &node.name;
            let constructor = &node.constructor;

            quote! {
                let #name = graph.add_node(#constructor);
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
            fields.push(quote! { #name });
        }

        quote! { #(#fields),* }
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
                fields.push(quote! { pub #field_name: #endpoints_type });
            }
        }

        let input_params = self.generate_input_params();
        let node_creation = self.generate_node_creation();
        let connections = self.generate_connections()?;
        let struct_init = self.generate_struct_init();

        Ok(quote! {
            #[allow(dead_code)]
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
