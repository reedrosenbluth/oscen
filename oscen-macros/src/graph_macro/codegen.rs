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
        if graph_def.compile_time {
            // Compile-time: Generate static struct with concrete node fields
            ctx.generate_static_struct(name)
        } else {
            // Runtime: Generate struct with Graph wrapper and endpoints
            ctx.generate_runtime_struct(name)
        }
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
    ///       VoiceAllocator<4> -> VoiceAllocatorEndpoints<4>
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
        let endpoints_ident = syn::Ident::new(&format!("{}Endpoints", type_name), type_name.span());

        // Preserve generic arguments from the original type
        let generic_args = &last_segment.arguments;

        if leading_segments.is_empty() {
            // Simple type like PolyBlepOscillator or VoiceAllocator<4>
            // Assume it comes from oscen crate and generate fully-qualified path
            quote! { ::oscen::#endpoints_ident #generic_args }
        } else {
            // Qualified type like oscen::PolyBlepOscillator
            quote! { #(#leading_segments)::* :: #endpoints_ident #generic_args }
        }
    }

    /// Construct the IO type from a node type
    /// E.g., PolyBlepOscillator -> PolyBlepOscillatorIO
    ///       TptFilter -> TptFilterIO
    #[allow(dead_code)]
    fn construct_io_type(node_type: &syn::Path) -> TokenStream {
        // For now, use the full node path and append IO to the final segment
        // This preserves the module path (e.g., oscen::PolyBlepOscillator becomes oscen::PolyBlepOscillatorIO)
        let mut path = node_type.clone();

        if let Some(last_seg) = path.segments.last_mut() {
            let node_name = &last_seg.ident;
            let io_name = syn::Ident::new(&format!("{}IO", node_name), node_name.span());
            last_seg.ident = io_name;
        }

        quote! { #path }
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
        let mut fields = vec![quote! { pub graph: ::oscen::Graph }];

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

    #[allow(dead_code)]
    fn generate_context_impl(&self) -> Result<TokenStream> {
        let input_params = self.generate_input_params();
        let output_params = self.generate_output_params();
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

                    // Create output parameters
                    #(#output_params)*

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

    fn generate_output_params(&self) -> Vec<TokenStream> {
        self.outputs.iter().map(|output| {
            let name = &output.name;

            match output.kind {
                EndpointKind::Stream => {
                    quote! {
                        let #name = {
                            let key = graph.allocate_endpoint(::oscen::graph::EndpointType::Stream);
                            ::oscen::StreamOutput::new(key)
                        };
                    }
                }
                EndpointKind::Value => {
                    quote! {
                        let #name = graph.value_param(0.0);
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        let #name = graph.event_param();
                    }
                }
            }
        }).collect()
    }

    fn generate_node_creation(&self) -> Vec<TokenStream> {
        self.nodes
            .iter()
            .flat_map(|node| {
                let name = &node.name;
                let constructor = &node.constructor;

                if let Some(array_size) = node.array_size {
                    // Generate multiple instances with indexed names
                    (0..array_size)
                        .map(|i| {
                            let indexed_name =
                                syn::Ident::new(&format!("{}_{}", name, i), name.span());
                            quote! {
                                let #indexed_name = graph.add_node(#constructor);
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    // Single instance
                    vec![quote! {
                        let #name = graph.add_node(#constructor);
                    }]
                }
            })
            .collect()
    }

    /// Generate static initialization for input parameters
    /// Note: We still need a minimal graph for endpoint allocation for inputs/outputs
    fn generate_static_input_params(&self) -> Vec<TokenStream> {
        self.inputs.iter().map(|input| {
            let name = &input.name;
            let default_val = input.default.as_ref();

            match input.kind {
                EndpointKind::Value => {
                    if let Some(default) = default_val {
                        quote! {
                            let #name = __temp_graph.value_param(#default);
                        }
                    } else {
                        quote! {
                            let #name = __temp_graph.value_param(0.0);
                        }
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        let #name = __temp_graph.event_param();
                    }
                }
                EndpointKind::Stream => {
                    quote! {
                        let #name = {
                            let key = __temp_graph.allocate_endpoint(::oscen::graph::EndpointType::Stream);
                            ::oscen::StreamInput::new(::oscen::graph::InputEndpoint::new(key))
                        };
                    }
                }
            }
        }).collect()
    }

    /// Generate static initialization for nodes (direct constructor calls)
    fn generate_static_node_init(&self) -> Vec<TokenStream> {
        self.nodes
            .iter()
            .map(|node| {
                let name = &node.name;
                let constructor = &node.constructor;

                if let Some(array_size) = node.array_size {
                    // Generate array initialization by repeating constructor
                    let constructors = vec![constructor; array_size];
                    quote! {
                        let #name = [#(#constructors),*];
                    }
                } else {
                    // Single node initialization
                    quote! {
                        let #name = #constructor;
                    }
                }
            })
            .collect()
    }

    fn generate_connections(&self) -> Result<Vec<TokenStream>> {
        if self.connections.is_empty() {
            return Ok(vec![]);
        }

        let mut temp_stmts = Vec::new(); // Temporary variable declarations
        let mut regular_connections = Vec::new(); // Connection expressions
        let mut output_assignments = Vec::new();
        let mut temp_counter = 0;

        for conn in &self.connections {
            // Check if destination is an output
            if let ConnectionExpr::Ident(dest_ident) = &conn.dest {
                if self.outputs.iter().any(|o| o.name == *dest_ident) {
                    // This is an output assignment with potential intermediate values
                    let (stmts, final_expr) =
                        self.generate_expr_with_temps(&conn.source, &mut temp_counter)?;
                    output_assignments.extend(stmts);
                    output_assignments.push(quote! {
                        let #dest_ident = #final_expr;
                    });
                    continue;
                }
            }

            // Check for array broadcasting pattern
            if let Some(expanded) = self.try_expand_array_broadcast(&conn.source, &conn.dest)? {
                regular_connections.extend(expanded);
                continue;
            }

            // Regular connection - extract temps from source to avoid nested mutable borrows
            let (source_stmts, source_expr) =
                self.generate_expr_with_temps(&conn.source, &mut temp_counter)?;
            temp_stmts.extend(source_stmts);

            let dest = self.generate_connection_expr(&conn.dest)?;
            regular_connections.push(quote! {
                #source_expr >> #dest
            });
        }

        let mut result = Vec::new();

        // Add temp variable declarations first
        result.extend(temp_stmts);

        // Then add connections
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

    /// Try to expand array broadcasting patterns:
    /// 1. Broadcast marker: `voice_allocator.voices() -> voice_handlers.note_on()`
    /// 2. Array-to-array: `voice_handlers.frequency() -> voices.frequency()`
    /// 3. Scalar-to-array: `cutoff -> voices.cutoff()`
    fn try_expand_array_broadcast(
        &self,
        source: &ConnectionExpr,
        dest: &ConnectionExpr,
    ) -> Result<Option<Vec<TokenStream>>> {
        // Pattern 1 & 2: Destination is a method call on an array
        if let ConnectionExpr::Method(dest_obj, dest_method, dest_args) = dest {
            if let ConnectionExpr::Ident(dest_base) = &**dest_obj {
                // Check if dest_base is an array node
                if let Some(dest_array_size) = self
                    .nodes
                    .iter()
                    .find(|n| n.name == *dest_base)
                    .and_then(|n| n.array_size)
                {
                    // Pattern 1: Broadcast marker (e.g., voices())
                    if let ConnectionExpr::Method(src_obj, src_method, _src_args) = source {
                        if let ConnectionExpr::Ident(src_base) = &**src_obj {
                            if src_method == "voices" {
                                // Generate N connections: src.voice(i) -> dest[i].method()
                                let mut connections = Vec::new();
                                for i in 0..dest_array_size {
                                    let src_indexed = quote! { #src_base.voice(#i) };
                                    let dest_indexed_name = syn::Ident::new(
                                        &format!("{}_{}", dest_base, i),
                                        dest_base.span(),
                                    );

                                    let dest_call = if dest_args.is_empty() {
                                        quote! { #dest_indexed_name.#dest_method }
                                    } else {
                                        quote! { #dest_indexed_name.#dest_method(#(#dest_args),*) }
                                    };

                                    connections.push(quote! {
                                        #src_indexed >> #dest_call
                                    });
                                }
                                return Ok(Some(connections));
                            }
                        }
                    }

                    // Pattern 2: Array-to-array (src is method on array, dest is method on array)
                    if let ConnectionExpr::Method(src_obj, src_method, src_args) = source {
                        if let ConnectionExpr::Ident(src_base) = &**src_obj {
                            if let Some(src_array_size) = self
                                .nodes
                                .iter()
                                .find(|n| n.name == *src_base)
                                .and_then(|n| n.array_size)
                            {
                                // Arrays must have the same size
                                if src_array_size == dest_array_size {
                                    let mut connections = Vec::new();
                                    for i in 0..src_array_size {
                                        let src_indexed_name = syn::Ident::new(
                                            &format!("{}_{}", src_base, i),
                                            src_base.span(),
                                        );
                                        let dest_indexed_name = syn::Ident::new(
                                            &format!("{}_{}", dest_base, i),
                                            dest_base.span(),
                                        );

                                        let src_call = if src_args.is_empty() {
                                            quote! { #src_indexed_name.#src_method }
                                        } else {
                                            quote! { #src_indexed_name.#src_method(#(#src_args),*) }
                                        };

                                        let dest_call = if dest_args.is_empty() {
                                            quote! { #dest_indexed_name.#dest_method }
                                        } else {
                                            quote! { #dest_indexed_name.#dest_method(#(#dest_args),*) }
                                        };

                                        connections.push(quote! {
                                            #src_call >> #dest_call
                                        });
                                    }
                                    return Ok(Some(connections));
                                }
                            }
                        }
                    }

                    // Pattern 3: Scalar-to-array (src is scalar, dest is method on array)
                    if let ConnectionExpr::Ident(src_ident) = source {
                        // Check if source is an input or output (scalar)
                        let is_scalar = self.inputs.iter().any(|i| i.name == *src_ident)
                            || self.outputs.iter().any(|o| o.name == *src_ident);

                        if is_scalar {
                            let mut connections = Vec::new();
                            for i in 0..dest_array_size {
                                let dest_indexed_name = syn::Ident::new(
                                    &format!("{}_{}", dest_base, i),
                                    dest_base.span(),
                                );

                                let dest_call = if dest_args.is_empty() {
                                    quote! { #dest_indexed_name.#dest_method }
                                } else {
                                    quote! { #dest_indexed_name.#dest_method(#(#dest_args),*) }
                                };

                                connections.push(quote! {
                                    #src_ident >> #dest_call
                                });
                            }
                            return Ok(Some(connections));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Generate an expression, extracting binary operations to temporary variables
    fn generate_expr_with_temps(
        &self,
        expr: &ConnectionExpr,
        counter: &mut usize,
    ) -> Result<(Vec<TokenStream>, TokenStream)> {
        match expr {
            ConnectionExpr::Binary(left, op, right) => {
                // Generate left side (might create temps)
                let (mut stmts, left_expr) = self.generate_expr_with_temps(left, counter)?;

                // Generate right side (might create temps)
                let (right_stmts, right_expr) = self.generate_expr_with_temps(right, counter)?;
                stmts.extend(right_stmts);

                // Create a temp variable for this binary operation
                let temp_name = syn::Ident::new(
                    &format!("__temp_{}", counter),
                    proc_macro2::Span::call_site(),
                );
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
            ConnectionExpr::Method(obj, method, args) => {
                // Check if this is a method call on an array node that needs summing
                if let ConnectionExpr::Ident(base_name) = &**obj {
                    if let Some(array_size) = self
                        .nodes
                        .iter()
                        .find(|n| n.name == *base_name)
                        .and_then(|n| n.array_size)
                    {
                        // Generate temps for each addition to avoid nested mutable borrows
                        let mut stmts = Vec::new();

                        // First element
                        let first_indexed_name =
                            syn::Ident::new(&format!("{}_{}", base_name, 0), base_name.span());

                        let mut sum_temp = if args.is_empty() {
                            quote! { #first_indexed_name.#method }
                        } else {
                            quote! { #first_indexed_name.#method(#(#args),*) }
                        };

                        // Add remaining elements, creating a temp for each addition
                        for i in 1..array_size {
                            let indexed_name =
                                syn::Ident::new(&format!("{}_{}", base_name, i), base_name.span());

                            let call_expr = if args.is_empty() {
                                quote! { #indexed_name.#method }
                            } else {
                                quote! { #indexed_name.#method(#(#args),*) }
                            };

                            let temp_name = syn::Ident::new(
                                &format!("__temp_{}", counter),
                                proc_macro2::Span::call_site(),
                            );
                            *counter += 1;

                            stmts.push(quote! {
                                let #temp_name = graph.add(#sum_temp, #call_expr);
                            });

                            sum_temp = quote! { #temp_name };
                        }

                        return Ok((stmts, sum_temp));
                    }
                }

                // Not an array sum, generate normally
                let expr_code = self.generate_connection_expr(expr)?;
                Ok((vec![], expr_code))
            }
            _ => {
                // No binary operations or special cases, generate normally
                let expr_code = self.generate_connection_expr(expr)?;
                Ok((vec![], expr_code))
            }
        }
    }

    fn generate_connection_expr(&self, expr: &ConnectionExpr) -> Result<TokenStream> {
        match expr {
            ConnectionExpr::Ident(ident) => Ok(quote! { #ident }),
            ConnectionExpr::ArrayIndex(array_expr, index) => {
                // For array[idx], we need to check if the base is an identifier
                // If it is, translate to base_idx (e.g., voices[0] -> voices_0)
                if let ConnectionExpr::Ident(base_name) = &**array_expr {
                    let indexed_name =
                        syn::Ident::new(&format!("{}_{}", base_name, index), base_name.span());
                    Ok(quote! { #indexed_name })
                } else {
                    // For more complex expressions, use actual array indexing
                    let array = self.generate_connection_expr(array_expr)?;
                    Ok(quote! { #array[#index] })
                }
            }
            ConnectionExpr::Method(obj, method, args) => {
                // NOTE: Array method summing is handled in generate_expr_with_temps
                // to properly extract operations into temps and avoid borrow checker issues.
                // This path should only be reached for non-array method calls.
                let obj_expr = self.generate_connection_expr(obj)?;
                if args.is_empty() {
                    // Field access (no parentheses)
                    Ok(quote! { #obj_expr.#method })
                } else {
                    // Method call with arguments
                    Ok(quote! { #obj_expr.#method(#(#args),*) })
                }
            }
            ConnectionExpr::Binary(left, op, right) => {
                let left_expr = self.generate_connection_expr(left)?;
                let right_expr = self.generate_connection_expr(right)?;

                match op {
                    BinaryOp::Mul => Ok(quote! { graph.multiply(#left_expr, #right_expr) }),
                    BinaryOp::Add => Ok(quote! { graph.add(#left_expr, #right_expr) }),
                    BinaryOp::Sub => Ok(quote! { graph.subtract(#left_expr, #right_expr) }),
                    BinaryOp::Div => Ok(quote! { graph.divide(#left_expr, #right_expr) }),
                }
            }
            ConnectionExpr::Literal(lit) => Ok(quote! { #lit }),
            ConnectionExpr::Call(func, args) => {
                let arg_exprs: Result<Vec<_>> = args
                    .iter()
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
        let _output_params = self.generate_output_params();
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

    #[allow(dead_code)]
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
                    .map(|i| syn::Ident::new(&format!("{}_{}", name, i), name.span()))
                    .collect();
                fields.push(quote! { #name: [#(#indexed_names),*] });
            } else {
                fields.push(quote! { #name });
            }
        }

        quote! { #(#fields),* }
    }

    /// Generate static struct initialization (includes sample_rate, nodes - no IO fields)
    fn generate_static_struct_init(&self) -> TokenStream {
        let mut fields = vec![quote! { sample_rate }];

        // Add input/output fields
        for input in &self.inputs {
            let name = &input.name;
            fields.push(quote! { #name });
        }

        for output in &self.outputs {
            let name = &output.name;
            fields.push(quote! { #name });
        }

        // Add node fields (no IO fields)
        for node in &self.nodes {
            let name = &node.name;
            fields.push(quote! { #name });
        }

        quote! { #(#fields),* }
    }

    /// Generate the Endpoints struct for a graph (e.g., VoiceEndpoints)
    fn generate_endpoints_struct(&self, graph_name: &syn::Ident) -> TokenStream {
        let endpoints_name =
            syn::Ident::new(&format!("{}Endpoints", graph_name), graph_name.span());

        let mut fields = vec![quote! { node_key: ::oscen::NodeKey }];

        let mut accessor_methods = Vec::new();

        // Add input fields and accessor methods
        for input in &self.inputs {
            let field_name = &input.name;
            let method_name = &input.name;
            let (ty, accessor_ty) = match input.kind {
                EndpointKind::Value => (
                    quote! { ::oscen::ValueInput },
                    quote! { ::oscen::ValueInput },
                ),
                EndpointKind::Event => (
                    quote! { ::oscen::EventInput },
                    quote! { ::oscen::EventInput },
                ),
                EndpointKind::Stream => (
                    quote! { ::oscen::StreamInput },
                    quote! { ::oscen::StreamInput },
                ),
            };
            fields.push(quote! { pub #field_name: #ty });
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
                    quote! { ::oscen::ValueOutput },
                ),
                EndpointKind::Event => (
                    quote! { ::oscen::EventOutput },
                    quote! { ::oscen::EventOutput },
                ),
                EndpointKind::Stream => (
                    quote! { ::oscen::StreamOutput },
                    quote! { ::oscen::StreamOutput },
                ),
            };
            fields.push(quote! { pub #field_name: #ty });
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
        let _return_expr = if let Some(first_output) = self.outputs.first() {
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

        // Generate code to route stream outputs from internal graph
        let mut stream_output_routing = Vec::new();
        let mut stream_output_idx = 0usize;
        for output in &self.outputs {
            if output.kind == EndpointKind::Stream {
                let field_name = &output.name;
                stream_output_routing.push(quote! {
                    #stream_output_idx => {
                        self.graph.get_value(&self.#field_name)
                    }
                });
                stream_output_idx += 1;
            }
        }

        // Generate code to route value outputs from internal graph
        let mut value_output_routing = Vec::new();
        let mut value_output_idx = 0usize;
        for output in &self.outputs {
            if output.kind == EndpointKind::Value {
                let field_name = &output.name;
                value_output_routing.push(quote! {
                    #value_output_idx => {
                        self.graph.get_value(&self.#field_name)
                            .map(::oscen::graph::types::ValueData::scalar)
                    }
                });
                value_output_idx += 1;
            }
        }

        // Generate code to route stream inputs to internal graph
        let mut stream_input_routing = Vec::new();
        let mut stream_input_idx = 0usize;
        for input in &self.inputs {
            if input.kind == EndpointKind::Stream {
                let field_name = &input.name;
                stream_input_routing.push(quote! {
                    #stream_input_idx => {
                        if let Some(state) = self.graph.endpoints.get_mut(self.#field_name.key()) {
                            state.set_scalar(value);
                        }
                    }
                });
                stream_input_idx += 1;
            }
        }

        // Generate NodeIO implementation for handling input/output routing
        let node_io_impl = quote! {
            impl ::oscen::NodeIO for #name {
                #[inline(always)]
                fn read_inputs<'a>(&mut self, context: &mut ::oscen::ProcessingContext<'a>) {
                    // Route external inputs to internal graph endpoints
                    #(#input_routing)*
                }

                #[inline(always)]
                fn get_stream_output(&self, index: usize) -> Option<f32> {
                    match index {
                        #(#stream_output_routing,)*
                        _ => None
                    }
                }

                #[inline(always)]
                fn set_stream_input(&mut self, index: usize, value: f32) {
                    match index {
                        #(#stream_input_routing,)*
                        _ => {}
                    }
                }

                #[inline(always)]
                fn get_value_output(&self, index: usize) -> Option<::oscen::graph::types::ValueData> {
                    match index {
                        #(#value_output_routing,)*
                        _ => None
                    }
                }
            }
        };

        // Generate SignalProcessor implementation
        let signal_processor_impl = quote! {
            impl ::oscen::SignalProcessor for #name {
                #[inline(always)]
                fn process(&mut self, _sample_rate: f32) {
                    // Process internal graph
                    let _ = self.graph.process();
                }
            }
        };

        quote! {
            #node_io_impl
            #signal_processor_impl
        }
    }

    /// Generate ProcessingNode implementation for graph
    fn generate_processing_node_impl(&self, name: &syn::Ident) -> TokenStream {
        let endpoints_name = syn::Ident::new(&format!("{}Endpoints", name), name.span());

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

    /// Generate a runtime struct with Graph wrapper and endpoints (compile_time: false)
    fn generate_runtime_struct(&self, name: &syn::Ident) -> Result<TokenStream> {
        let mut fields = vec![quote! { graph: ::oscen::Graph }];

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

        // Add node endpoint fields (using Endpoints types)
        for node in &self.nodes {
            let field_name = &node.name;
            if let Some(node_type) = &node.node_type {
                let endpoints_type = Self::construct_endpoints_type(node_type);
                if let Some(array_size) = node.array_size {
                    fields.push(quote! { pub #field_name: [#endpoints_type; #array_size] });
                } else {
                    fields.push(quote! { pub #field_name: #endpoints_type });
                }
            }
        }

        let input_params = self.generate_input_params();
        let output_params = self.generate_output_params();
        let node_creation = self.generate_node_creation();
        let connections = self.generate_connections()?;
        let struct_init = self.generate_struct_init();

        // Generate trait implementations
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

                    // Create output parameters
                    #(#output_params)*

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

            // Generate Endpoints struct (for use as a ProcessingNode)
            #endpoints_struct

            // Generate SignalProcessor implementation (for use as a ProcessingNode)
            #signal_processor_impl

            // Generate ProcessingNode implementation (for adding to other Graphs)
            #processing_node_impl
        })
    }

    /// Generate a compile-time optimized struct with concrete node fields (compile_time: true)
    /// Extract the root node identifier from a connection expression
    /// For example: osc.output -> "osc", filter.cutoff -> "filter"
    fn extract_root_node(expr: &ConnectionExpr) -> Option<&syn::Ident> {
        match expr {
            ConnectionExpr::Ident(ident) => Some(ident),
            ConnectionExpr::Method(base, _, _) => Self::extract_root_node(base),
            ConnectionExpr::ArrayIndex(base, _) => Self::extract_root_node(base),
            ConnectionExpr::Binary(left, _, _) => Self::extract_root_node(left),
            ConnectionExpr::Literal(_) | ConnectionExpr::Call(_, _) => None,
        }
    }

    /// Build dependency map: node -> list of nodes it depends on
    fn build_dependency_map(&self) -> std::collections::HashMap<syn::Ident, Vec<syn::Ident>> {
        use std::collections::HashMap;

        let mut deps: HashMap<syn::Ident, Vec<syn::Ident>> = HashMap::new();

        // Initialize all nodes with empty dependency lists
        for node in &self.nodes {
            deps.insert(node.name.clone(), Vec::new());
        }

        // Build dependencies from connections: dest depends on source
        for conn in &self.connections {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                    // Skip if source or dest is not a node (could be input/output)
                    if deps.contains_key(source_node) && deps.contains_key(dest_node) {
                        // dest depends on source
                        deps.get_mut(dest_node).unwrap().push(source_node.clone());
                    }
                }
            }
        }

        deps
    }

    /// Check if a node type allows feedback (like Delay nodes)
    fn is_feedback_allowing_node(node_type: &Option<syn::Path>) -> bool {
        if let Some(path) = node_type {
            // Check if the type name ends with "Delay"
            if let Some(last_segment) = path.segments.last() {
                let type_name = last_segment.ident.to_string();
                return type_name.contains("Delay");
            }
        }
        false
    }

    /// Perform topological sort on nodes using the generic algorithm
    fn topological_sort_nodes(&self) -> Result<Vec<syn::Ident>> {
        let deps = self.build_dependency_map();

        // Collect all node names
        let nodes: Vec<syn::Ident> = self.nodes.iter().map(|n| n.name.clone()).collect();

        // Create closures for the generic topological_sort function
        let get_dependencies =
            |node: &syn::Ident| -> Vec<syn::Ident> { deps.get(node).cloned().unwrap_or_default() };

        let allows_feedback = |node: &syn::Ident| -> bool {
            self.nodes
                .iter()
                .find(|n| &n.name == node)
                .map(|n| Self::is_feedback_allowing_node(&n.node_type))
                .unwrap_or(false)
        };

        // We can't directly call oscen::graph::topology::topological_sort at compile time,
        // so we'll implement a simplified version inline for now
        // TODO: Extract this into a shared compile-time sorting function

        use std::collections::{HashMap, HashSet};

        // Build adjacency map: node -> nodes that depend on it
        let mut adjacency: HashMap<syn::Ident, Vec<syn::Ident>> = HashMap::new();
        for node in &nodes {
            adjacency.insert(node.clone(), Vec::new());
        }

        for node in &nodes {
            let dependencies = get_dependencies(node);
            for dep in dependencies {
                adjacency
                    .entry(dep.clone())
                    .or_insert_with(Vec::new)
                    .push(node.clone());
            }
        }

        // Identify feedback-allowing nodes
        let feedback_nodes: HashSet<syn::Ident> = nodes
            .iter()
            .filter(|n| allows_feedback(n))
            .cloned()
            .collect();

        // For sorting, remove outgoing edges from feedback nodes to break cycles
        let mut sort_adjacency = adjacency.clone();
        for feedback_node in &feedback_nodes {
            sort_adjacency.insert(feedback_node.clone(), Vec::new());
        }

        // Perform DFS-based topological sort
        let mut sorted = Vec::with_capacity(nodes.len());
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();

        fn visit(
            node: syn::Ident,
            adjacency: &HashMap<syn::Ident, Vec<syn::Ident>>,
            visited: &mut HashSet<syn::Ident>,
            recursion_stack: &mut HashSet<syn::Ident>,
            sorted: &mut Vec<syn::Ident>,
        ) -> Result<()> {
            let node_str = node.to_string();

            if recursion_stack.contains(&node) {
                return Err(syn::Error::new(
                    node.span(),
                    format!("Cycle detected involving node '{}'", node_str),
                ));
            }

            if visited.contains(&node) {
                return Ok(());
            }

            visited.insert(node.clone());
            recursion_stack.insert(node.clone());

            if let Some(neighbors) = adjacency.get(&node) {
                for neighbor in neighbors {
                    visit(
                        neighbor.clone(),
                        adjacency,
                        visited,
                        recursion_stack,
                        sorted,
                    )?;
                }
            }

            recursion_stack.remove(&node);
            sorted.push(node);

            Ok(())
        }

        for node in &nodes {
            if !visited.contains(node) {
                visit(
                    node.clone(),
                    &sort_adjacency,
                    &mut visited,
                    &mut recursion_stack,
                    &mut sorted,
                )?;
            }
        }

        // Reverse to get dependency order (dependencies first)
        sorted.reverse();

        Ok(sorted)
    }

    /// Extract the method name from a connection expression
    /// For example: osc.output -> Some("output"), filter.cutoff -> Some("cutoff")
    fn extract_endpoint_field(expr: &ConnectionExpr) -> Option<&syn::Ident> {
        match expr {
            ConnectionExpr::Method(_, method, _) => Some(method),
            _ => None,
        }
    }

    /// Generate connection assignments for a specific node
    /// Returns assignments that should be executed before processing this node
    fn generate_connection_assignments_for_node(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut assignments = Vec::new();

        // Find all connections where this node is the destination
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    // This connection feeds into the current node
                    if let Some(source_node) = Self::extract_root_node(&conn.source) {
                        if let Some(source_field) = Self::extract_endpoint_field(&conn.source) {
                            if let Some(dest_field) = Self::extract_endpoint_field(&conn.dest) {
                                // Generate: self.dest_node.dest_field = self.source_node.source_field;
                                assignments.push(quote! {
                                    self.#dest_node.#dest_field = self.#source_node.#source_field;
                                });
                            }
                        }
                    }
                }
            }
        }

        assignments
    }

    /// Generate the static process() method for compile-time graphs
    fn generate_static_process(&self) -> Result<TokenStream> {
        let sorted_nodes = self.topological_sort_nodes()?;

        // Generate process calls for each node in topological order
        let mut process_body = Vec::new();

        for node_name in &sorted_nodes {
            // First, generate connection assignments that feed into this node
            let assignments = self.generate_connection_assignments_for_node(node_name);
            process_body.extend(assignments);

            // Then generate the direct process call (bypasses trait dispatch for inlining)
            process_body.push(quote! {
                self.#node_name.process(self.sample_rate);
            });
        }

        // Determine what to return - look for outputs or the last node's output
        let return_value = if let Some(output) = self.outputs.first() {
            let output_name = &output.name;
            quote! { self.#output_name }
        } else if let Some(last_node) = sorted_nodes.last() {
            // Return the output field of the last processed node directly
            quote! { self.#last_node.output }
        } else {
            quote! { 0.0 }
        };

        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) -> f32 {
                use ::oscen::SignalProcessor as _;
                #(#process_body)*

                #return_value
            }
        })
    }

    fn generate_static_struct(&self, name: &syn::Ident) -> Result<TokenStream> {
        let mut fields = vec![quote! { sample_rate: f32 }];

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

        // Add concrete node fields (no IO structs)
        for node in &self.nodes {
            let field_name = &node.name;
            if let Some(node_type) = &node.node_type {
                if let Some(array_size) = node.array_size {
                    // Array of nodes
                    fields.push(quote! { pub #field_name: [#node_type; #array_size] });
                } else {
                    // Single node
                    fields.push(quote! { pub #field_name: #node_type });
                }
            }
        }

        let input_params = self.generate_static_input_params();
        let node_init = self.generate_static_node_init();
        let struct_init = self.generate_static_struct_init();

        // For compile-time graphs, generate a static process() method
        let process_method = self.generate_static_process()?;

        Ok(quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #name {
                #(#fields),*
            }

            impl #name {
                #[allow(unused_variables, unused_mut)]
                pub fn new(sample_rate: f32) -> Self {
                    // Create temporary graph for input/output endpoint allocation
                    let mut __temp_graph = ::oscen::Graph::new(sample_rate);

                    // Initialize input parameters (requires graph for endpoint allocation)
                    #(#input_params)*

                    // Initialize nodes (direct instantiation)
                    #(#node_init)*

                    Self {
                        #struct_init
                    }
                }

                #process_method
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
            Self::Binary(left, op, right) => Self::Binary(left.clone(), *op, right.clone()),
            Self::Literal(lit) => Self::Literal(lit.clone()),
            Self::Call(func, args) => Self::Call(func.clone(), args.clone()),
        }
    }
}
