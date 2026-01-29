use super::ast::*;
use super::type_check::TypeContext;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, Result};

pub fn generate(graph_def: &GraphDef) -> Result<TokenStream> {
    let mut ctx = CodegenContext::new();

    // Collect all declarations
    for item in &graph_def.items {
        ctx.collect_item(item)?;
    }

    // Validate connections
    ctx.validate_connections()?;

    // Static graphs require a name
    if let Some(name) = &graph_def.name {
        ctx.generate_static_struct(name)
    } else {
        Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "graph! macro requires a name (anonymous graphs are no longer supported)",
        ))
    }
}

struct CodegenContext {
    inputs: Vec<InputDecl>,
    outputs: Vec<OutputDecl>,
    nodes: Vec<NodeDecl>,
    connections: Vec<ConnectionStmt>,
    nih_params: bool,
}

impl CodegenContext {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            nih_params: false,
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
            GraphItem::NihParams => {
                self.nih_params = true;
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

        // Infer node endpoint types from connections for type compatibility checking
        // Note: This inference is no longer needed for codegen (process_event_inputs() is called uniformly)
        // but we keep it for type compatibility validation
        self.infer_node_endpoint_types(&mut type_ctx);

        // Validate each connection for type compatibility
        for conn in &self.connections {
            // Validate source and destination independently
            type_ctx.validate_source(&conn.source)?;
            type_ctx.validate_destination(&conn.dest)?;

            // Validate type compatibility (stream/value/event)
            type_ctx.validate_connection(&conn.source, &conn.dest)?;
        }

        Ok(())
    }

    /// Infer node endpoint types from connections
    /// When we see `graph_input -> node.endpoint()`, we can infer endpoint's type from graph_input
    /// Runs iteratively until no new types can be inferred (fixed-point algorithm)
    fn infer_node_endpoint_types(&self, type_ctx: &mut TypeContext) {
        // Iterate until no new types are discovered (fixed-point)
        // This allows types to propagate through chains: input -> node1.x -> node2.y -> output
        let mut changed = true;
        let max_iterations = self.connections.len() + 1; // Safety limit
        let mut iteration = 0;

        while changed && iteration < max_iterations {
            changed = false;
            iteration += 1;

            for conn in &self.connections {
                // Special handling for voice array markers (like .voices)
                // These connections indicate event routing
                if let Some(source_node) = Self::extract_root_node(&conn.source) {
                    if let Some(source_endpoint) = Self::extract_endpoint_field(&conn.source) {
                        if source_endpoint == "voices" {
                            // This is a voice array marker
                            // Mark both source and destination as event endpoints
                            let source_key = (source_node.to_string(), source_endpoint.to_string());
                            if type_ctx.get_node_endpoint_type(&source_key.0, &source_key.1).is_none() {
                                type_ctx.register_node_endpoint(&source_key.0, &source_key.1, EndpointKind::Event);
                                changed = true;
                            }

                            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                                if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                                    let dest_key = (dest_node.to_string(), dest_endpoint.to_string());
                                    if type_ctx.get_node_endpoint_type(&dest_key.0, &dest_key.1).is_none() {
                                        type_ctx.register_node_endpoint(&dest_key.0, &dest_key.1, EndpointKind::Event);
                                        changed = true;
                                    }
                                }
                            }
                            continue; // Skip normal type inference for this connection
                        }
                    }
                }

                // Get source type
                let source_type = type_ctx.infer_type(&conn.source);

                // If destination is a node method call, try to register its type
                if let Some(node_name) = Self::extract_root_node(&conn.dest) {
                    if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                        if let Some(kind) = source_type {
                            // Check if this is a new registration
                            let key = (node_name.to_string(), endpoint_name.to_string());
                            if type_ctx.get_node_endpoint_type(&key.0, &key.1).is_none() {
                                type_ctx.register_node_endpoint(&key.0, &key.1, kind);
                                changed = true;
                            }
                        }
                    }
                }

                // If source is a node method call, try to register its type from destination
                if let Some(node_name) = Self::extract_root_node(&conn.source) {
                    if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.source) {
                        let dest_type = type_ctx.infer_type(&conn.dest);
                        if let Some(kind) = dest_type {
                            // Check if this is a new registration
                            let key = (node_name.to_string(), endpoint_name.to_string());
                            if type_ctx.get_node_endpoint_type(&key.0, &key.1).is_none() {
                                type_ctx.register_node_endpoint(&key.0, &key.1, kind);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    // ========== Static Graph Parameter Generation ==========

    fn generate_static_input_params(&self) -> Vec<TokenStream> {
        self.inputs.iter().map(|input| {
            let name = &input.name;
            let default_val = input.default.as_ref();

            match input.kind {
                EndpointKind::Value => {
                    if let Some(default) = default_val {
                        quote! {
                            let #name = #default;
                        }
                    } else {
                        quote! {
                            let #name = 0.0;
                        }
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        let #name = ::oscen::graph::StaticEventQueue::new();
                    }
                }
                EndpointKind::Stream => {
                    // Static graphs: stream inputs are plain f32, initialized to 0.0
                    quote! {
                        let #name = 0.0f32;
                    }
                }
            }
        }).collect()
    }

    /// Generate static initialization for output parameters
    /// For static graphs, outputs store actual values (f32) not endpoint wrappers
    fn generate_static_output_params(&self) -> Vec<TokenStream> {
        self.outputs.iter().map(|output| {
            let name = &output.name;

            match output.kind {
                EndpointKind::Stream => {
                    quote! {
                        let #name = 0.0f32;
                    }
                }
                EndpointKind::Value => {
                    quote! {
                        let #name = 0.0f32;
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        let #name = ::oscen::graph::StaticEventQueue::new();
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
                // For static graphs:
                // - If constructor is a path (Type), call Type::new() (Pattern 2)
                // - If constructor is already a call, use it as-is
                let constructor = match &node.constructor {
                    Expr::Path(path) => {
                        // Pattern 2: call new() without arguments
                        // init(sample_rate) will be called later
                        quote! { #path::new() }
                    },
                    Expr::Call(_) => {
                        let expr = &node.constructor;
                        quote! { #expr }
                    },
                    _ => {
                        let expr = &node.constructor;
                        quote! { #expr }
                    }
                };

                if let Some(array_size) = node.array_size {
                    // Generate array initialization by repeating constructor
                    let constructors = vec![constructor.clone(); array_size];
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

        // Note: Graph-level event storage is no longer generated
        // Nodes own their own EventInput/EventOutput storage

        quote! { #(#fields),* }
    }

    // ========== Static Graph Generation ==========
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

    /// Convert a ConnectionExpr to a TokenStream that evaluates it.
    /// Handles binary expressions, method calls, identifiers, etc.
    fn connection_expr_to_tokens(&self, expr: &ConnectionExpr) -> TokenStream {
        match expr {
            ConnectionExpr::Ident(ident) => {
                quote! { self.#ident }
            }
            ConnectionExpr::Method(base, method, _args) => {
                let base_tokens = self.connection_expr_to_tokens(base);
                quote! { #base_tokens.#method }
            }
            ConnectionExpr::ArrayIndex(base, idx) => {
                let base_tokens = self.connection_expr_to_tokens(base);
                quote! { #base_tokens[#idx] }
            }
            ConnectionExpr::Binary(left, op, right) => {
                let left_tokens = self.connection_expr_to_tokens(left);
                let right_tokens = self.connection_expr_to_tokens(right);
                let op_token = match op {
                    BinaryOp::Add => quote! { + },
                    BinaryOp::Sub => quote! { - },
                    BinaryOp::Mul => quote! { * },
                    BinaryOp::Div => quote! { / },
                };
                quote! { (#left_tokens #op_token #right_tokens) }
            }
            ConnectionExpr::Literal(lit) => {
                quote! { #lit }
            }
            ConnectionExpr::Call(func, args) => {
                let arg_tokens: Vec<_> = args.iter()
                    .map(|arg| self.connection_expr_to_tokens(arg))
                    .collect();
                quote! { #func(#(#arg_tokens),*) }
            }
        }
    }

    fn get_node_array_size(&self, name: &syn::Ident) -> Option<usize> {
        self.nodes
            .iter()
            .find(|n| n.name == *name)
            .and_then(|n| n.array_size)
    }

    /// Generate connection assignments for a specific node
    /// Returns assignments that should be executed before processing this node
    /// Uses trait-based dispatch (ConnectEndpoints) for ALL connection types,
    /// eliminating the need for type inference to determine event vs stream connections.
    fn generate_connection_assignments_for_node(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut assignments = Vec::new();

        // Find all connections where this node is the destination
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    // This connection feeds into the current node
                    if let Some(source_ident) = Self::extract_root_node(&conn.source) {
                        let source_field = Self::extract_endpoint_field(&conn.source);

                        if let Some(dest_field) = Self::extract_endpoint_field(&conn.dest) {
                            // Check if source is a graph input (not a node)
                            let source_is_graph_input = self.inputs.iter().any(|i| i.name == *source_ident);

                            // Skip voice array marker connections (like .voices -> array.endpoint)
                            // These have special routing handled by the array output node
                            if let Some(ref field) = source_field {
                                if *field == "voices" {
                                    // For voice arrays, the routing is done element-by-element
                                    // from source[i] to dest[i]
                                    if let Some(dest_array_size) = self.get_node_array_size(dest_node) {
                                        assignments.push(quote! {
                                            for i in 0..#dest_array_size {
                                                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                                    &self.#source_ident.voices[i],
                                                    &mut self.#dest_node[i].#dest_field
                                                );
                                            }
                                        });
                                    }
                                    continue;
                                }
                            }

                            let dest_array_size = self.get_node_array_size(dest_node);
                            let source_array_size = if source_is_graph_input {
                                None  // Graph inputs are never arrays
                            } else {
                                self.get_node_array_size(source_ident)
                            };

                            // Construct source expression part (field access or just node/input name)
                            let source_access = if let Some(field) = source_field {
                                quote! { .#field }
                            } else {
                                quote! {}
                            };

                            match (dest_array_size, source_array_size) {
                                (Some(dest_size), Some(src_size)) => {
                                    // Array-to-Array connection using trait dispatch
                                    if dest_size == src_size {
                                        assignments.push(quote! {
                                            for i in 0..#dest_size {
                                                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                                    &self.#source_ident[i] #source_access,
                                                    &mut self.#dest_node[i].#dest_field
                                                );
                                            }
                                        });
                                    } else {
                                        // Mismatched sizes - assuming 1-to-1 for min length
                                        let min_size = std::cmp::min(dest_size, src_size);
                                        assignments.push(quote! {
                                            for i in 0..#min_size {
                                                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                                    &self.#source_ident[i] #source_access,
                                                    &mut self.#dest_node[i].#dest_field
                                                );
                                            }
                                        });
                                    }
                                }
                                (Some(dest_size), None) => {
                                    // Scalar-to-Array broadcasting using trait dispatch
                                    assignments.push(quote! {
                                        for i in 0..#dest_size {
                                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                                &self.#source_ident #source_access,
                                                &mut self.#dest_node[i].#dest_field
                                            );
                                        }
                                    });
                                }
                                (None, Some(_)) => {
                                    // Array-to-Scalar reduction (Summing)
                                    if let Some(field) = source_field {
                                        assignments.push(quote! {
                                            self.#dest_node.#dest_field = self.#source_ident.iter().map(|n| n.#field).sum();
                                        });
                                    } else {
                                        assignments.push(quote! {
                                            self.#dest_node.#dest_field = self.#source_ident.iter().sum();
                                        });
                                    }
                                }
                                (None, None) => {
                                    // Scalar-to-Scalar using trait dispatch
                                    assignments.push(quote! {
                                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                            &self.#source_ident #source_access,
                                            &mut self.#dest_node.#dest_field
                                        );
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        assignments
    }

    /// Generate the static process() method for compile-time graphs
    /// Uses trait-based dispatch for ALL connection types, eliminating the need for
    /// type inference to distinguish events from streams.
    /// Now calls process_event_inputs() uniformly on all nodes, removing the need
    /// for type inference to identify event endpoints.
    fn generate_static_process(&self) -> Result<TokenStream> {
        let sorted_nodes = self.topological_sort_nodes()?;

        // Generate process calls for each node in topological order
        let mut process_body = Vec::new();

        for node_name in &sorted_nodes {
            // First, generate connection assignments that feed into this node
            // Uses trait dispatch for all types (events, streams, values)
            let assignments = self.generate_connection_assignments_for_node(node_name);
            process_body.extend(assignments);

            // Generate process call - now uniformly calls process_event_inputs() + process()
            // This eliminates the need for type inference to identify event endpoints
            if let Some(array_size) = self.get_node_array_size(node_name) {
                // Array node: process each element
                process_body.push(quote! {
                    for i in 0..#array_size {
                        self.#node_name[i].process_event_inputs();
                        self.#node_name[i].process();
                    }
                });
            } else {
                // Scalar node: process_event_inputs() then process()
                process_body.push(quote! {
                    self.#node_name.process_event_inputs();
                    self.#node_name.process();
                });
            }
        }

        // Generate assignments for connections to outputs
        for conn in &self.connections {
            if let Some(dest_ident) = Self::extract_root_node(&conn.dest) {
                // Check if destination is a graph output
                if let Some(output_decl) = self.outputs.iter().find(|o| o.name == *dest_ident) {
                    // This connection targets an output
                    // Check if source is a simple node.field pattern (for special array handling)
                    let source_node = Self::extract_root_node(&conn.source);
                    let source_field = Self::extract_endpoint_field(&conn.source);
                    let is_simple_source = source_node.is_some() && source_field.is_some();

                    match output_decl.kind {
                        EndpointKind::Stream | EndpointKind::Value => {
                            if is_simple_source {
                                let source_node = source_node.unwrap();
                                let source_field = source_field.unwrap();
                                if let Some(_src_array_size) = self.get_node_array_size(source_node) {
                                    // Array-to-Output: Summing
                                    process_body.push(quote! {
                                        self.#dest_ident = self.#source_node.iter().map(|n| n.#source_field).sum();
                                    });
                                } else {
                                    // Scalar-to-Output (simple case)
                                    process_body.push(quote! {
                                        self.#dest_ident = self.#source_node.#source_field;
                                    });
                                }
                            } else {
                                // Complex expression (binary ops, etc.) - evaluate and assign
                                let source_tokens = self.connection_expr_to_tokens(&conn.source);
                                process_body.push(quote! {
                                    self.#dest_ident = #source_tokens;
                                });
                            }
                        }
                        EndpointKind::Event => {
                            // Events only support simple node.field sources
                            if is_simple_source {
                                let source_node = source_node.unwrap();
                                let source_field = source_field.unwrap();
                                if let Some(array_size) = self.get_node_array_size(source_node) {
                                    // Array event source: copy all events from each element
                                    process_body.push(quote! {
                                        self.#dest_ident.clear();
                                        for i in 0..#array_size {
                                            for event in self.#source_node[i].#source_field.iter() {
                                                let _ = self.#dest_ident.try_push(event.clone());
                                            }
                                        }
                                    });
                                } else {
                                    // Scalar event source: use trait dispatch
                                    process_body.push(quote! {
                                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                            &self.#source_node.#source_field,
                                            &mut self.#dest_ident
                                        );
                                    });
                                }
                            }
                            // Note: Binary expressions for events don't make sense, so we skip them
                        }
                    }
                }
            }
        }

        // Generate event queue clearing for graph inputs/outputs
        let mut event_clearing = Vec::new();
        for input in &self.inputs {
            if input.kind == EndpointKind::Event {
                let field_name = &input.name;
                event_clearing.push(quote! {
                    self.#field_name.clear();
                });
            }
        }
        for output in &self.outputs {
            if output.kind == EndpointKind::Event {
                let field_name = &output.name;
                event_clearing.push(quote! {
                    self.#field_name.clear();
                });
            }
        }

        // Generate process() method with no return value
        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) {
                use ::oscen::SignalProcessor as _;

                #(#process_body)*

                // Clear event queues after processing
                #(#event_clearing)*
            }
        })
    }

    /// Generate event handler methods for static graphs
    /// This allows static graphs to be used as nested nodes in other graphs
    fn generate_static_event_handler_methods(&self) -> Vec<TokenStream> {
        let mut methods = Vec::new();

        // For each event input, generate a handle_{name}_events() method
        for input in &self.inputs {
            if input.kind == EndpointKind::Event {
                let endpoint_name = &input.name;
                let method_name = syn::Ident::new(
                    &format!("handle_{}_events", endpoint_name),
                    endpoint_name.span()
                );

                // Generate method that copies events to this graph's own input queue
                // The process() method will then route them to internal nodes
                methods.push(quote! {
                    pub fn #method_name(
                        &mut self,
                        events: &::oscen::graph::StaticEventQueue,
                    ) {
                        // Copy events to this graph's input queue
                        // process() will route them to internal nodes
                        self.#endpoint_name.clear();
                        for event in events.iter() {
                            let _ = self.#endpoint_name.try_push(event.clone());
                        }
                    }
                });
            }
        }

        methods
    }

    /// Generate get_stream_output() method for static graphs
    fn generate_static_get_stream_output(&self) -> TokenStream {
        // Generate match arms for each stream output
        let mut match_arms = Vec::new();
        let mut output_idx = 0usize;

        for output in &self.outputs {
            if output.kind == EndpointKind::Stream {
                let field_name = &output.name;
                match_arms.push(quote! {
                    #output_idx => Some(self.#field_name)
                });
                output_idx += 1;
            }
        }

        quote! {
            #[inline(always)]
            pub fn get_stream_output(&self, index: usize) -> Option<f32> {
                match index {
                    #(#match_arms,)*
                    _ => None
                }
            }
        }
    }

    /// Generate clear_event_outputs() method for graph types.
    /// This allows graphs to be nested as nodes in other graphs.
    fn generate_static_clear_event_outputs(&self) -> TokenStream {
        let mut clear_stmts = Vec::new();

        // Clear graph-level event output fields
        for output in &self.outputs {
            if output.kind == EndpointKind::Event {
                let field_name = &output.name;
                clear_stmts.push(quote! {
                    self.#field_name.clear();
                });
            }
        }

        quote! {
            /// Clear all event outputs before handlers run.
            /// Called by outer graphs when this graph is used as a nested node.
            #[inline]
            pub fn clear_event_outputs(&mut self) {
                #(#clear_stmts)*
            }
        }
    }

    /// Generate process_event_inputs() method for graph types.
    /// This allows graphs to be nested as nodes in other graphs with uniform event processing.
    fn generate_static_process_event_inputs(&self) -> TokenStream {
        // For graphs, process_event_inputs() just needs to clear outputs
        // The graph-level event inputs get routed to internal nodes during process()
        // via the connection assignments
        quote! {
            /// Process all event inputs: clear outputs before handlers run.
            /// Called by outer graphs when this graph is used as a nested node.
            /// The graph-level event inputs get routed to internal nodes during process().
            #[inline]
            pub fn process_event_inputs(&mut self) {
                self.clear_event_outputs();
            }
        }
    }

    // ========== NIH-plug Parameter Generation ==========

    /// Generate the NIH-plug params struct and its implementations
    /// Only called when `nih_params` flag is set and feature is enabled
    fn generate_nih_params_struct(&self, graph_name: &syn::Ident) -> TokenStream {
        let params_name = syn::Ident::new(
            &format!("{}Params", graph_name),
            graph_name.span(),
        );

        // Collect value inputs for parameter generation
        let value_inputs: Vec<_> = self.inputs.iter()
            .filter(|input| input.kind == EndpointKind::Value)
            .collect();

        // Generate field definitions
        let param_fields: Vec<_> = value_inputs.iter().map(|input| {
            let field_name = &input.name;
            let id_string = field_name.to_string();
            quote! {
                #[id = #id_string]
                pub #field_name: ::nih_plug::prelude::FloatParam
            }
        }).collect();

        // Generate Default impl with FloatParam constructors
        let param_defaults: Vec<_> = value_inputs.iter().map(|input| {
            let field_name = &input.name;
            let display_name = input.spec.as_ref()
                .and_then(|s| s.display_name.clone())
                .unwrap_or_else(|| {
                    // Convert snake_case to Title Case
                    field_name.to_string()
                        .split('_')
                        .map(|word| {
                            let mut chars = word.chars();
                            match chars.next() {
                                None => String::new(),
                                Some(first) => first.to_uppercase().chain(chars).collect(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                });

            let default_val = input.default.as_ref()
                .map(|expr| quote! { #expr })
                .unwrap_or_else(|| quote! { 0.0 });

            // Build the FloatRange
            let range_expr = if let Some(spec) = &input.spec {
                if let Some(range) = &spec.range {
                    let min = &range.min;
                    let max = &range.max;
                    if let Some(center) = &spec.center {
                        quote! {
                            ::nih_plug::prelude::FloatRange::Skewed {
                                min: #min,
                                max: #max,
                                factor: ::nih_plug::prelude::FloatRange::skew_factor(#center),
                            }
                        }
                    } else {
                        quote! {
                            ::nih_plug::prelude::FloatRange::Linear {
                                min: #min,
                                max: #max,
                            }
                        }
                    }
                } else {
                    // No range specified, default to 0..1
                    quote! {
                        ::nih_plug::prelude::FloatRange::Linear {
                            min: 0.0,
                            max: 1.0,
                        }
                    }
                }
            } else {
                // No spec at all, default to 0..1
                quote! {
                    ::nih_plug::prelude::FloatRange::Linear {
                        min: 0.0,
                        max: 1.0,
                    }
                }
            };

            // Build the FloatParam with optional modifiers
            let mut param_builder = quote! {
                ::nih_plug::prelude::FloatParam::new(
                    #display_name,
                    #default_val,
                    #range_expr,
                )
            };

            // Add smoother (default to 50ms if not specified)
            let smoother_ms = input.spec.as_ref()
                .and_then(|s| s.smoother.clone());
            if let Some(smoother_val) = smoother_ms {
                param_builder = quote! {
                    #param_builder
                        .with_smoother(::nih_plug::prelude::SmoothingStyle::Linear(#smoother_val))
                };
            } else {
                // Default 50ms smoothing for all NIH params
                param_builder = quote! {
                    #param_builder
                        .with_smoother(::nih_plug::prelude::SmoothingStyle::Linear(50.0))
                };
            }

            // Add optional unit
            if let Some(spec) = &input.spec {
                if let Some(unit) = &spec.unit {
                    // Prepend space to unit for proper display (e.g., "Hz" -> " Hz")
                    let unit_with_space = format!(" {}", unit);
                    param_builder = quote! {
                        #param_builder
                            .with_unit(#unit_with_space)
                    };
                }

                // Add optional step size
                if let Some(step) = &spec.step {
                    param_builder = quote! {
                        #param_builder
                            .with_step_size(#step)
                    };
                }
            }

            quote! {
                #field_name: #param_builder
            }
        }).collect();

        // Generate sync_to method
        let sync_assignments: Vec<_> = value_inputs.iter().map(|input| {
            let field_name = &input.name;
            quote! {
                graph.#field_name = self.#field_name.smoothed.next();
            }
        }).collect();

        quote! {
            #[derive(::nih_plug::prelude::Params)]
            pub struct #params_name {
                #(#param_fields),*
            }

            impl Default for #params_name {
                fn default() -> Self {
                    Self {
                        #(#param_defaults),*
                    }
                }
            }

            impl #params_name {
                /// Sync all smoothed parameter values to the graph (call per sample)
                #[inline(always)]
                pub fn sync_to(&self, graph: &mut #graph_name) {
                    #(#sync_assignments)*
                }
            }
        }
    }

    fn generate_static_struct(&self, name: &syn::Ident) -> Result<TokenStream> {
        let mut fields = vec![quote! { sample_rate: f32 }];

        // Add input fields
        for input in &self.inputs {
            let field_name = &input.name;
            let ty = match input.kind {
                EndpointKind::Value => quote! { f32 },
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
                EndpointKind::Stream => quote! { f32 },  // Static graphs use plain f32 for stream inputs
            };
            fields.push(quote! { pub #field_name: #ty });
        }

        // Add output fields (store actual values for static graphs)
        for output in &self.outputs {
            let field_name = &output.name;
            let ty = match output.kind {
                EndpointKind::Stream => quote! { f32 },  // Store actual f32 value
                EndpointKind::Value => quote! { f32 },   // Simplified: only scalar values for now
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
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

        // Note: Graph-level event storage is no longer generated
        // Nodes own their own EventInput/EventOutput storage, and trait-based dispatch
        // (ConnectEndpoints) handles routing between them.

        let input_params = self.generate_static_input_params();
        let output_params = self.generate_static_output_params();
        let node_init = self.generate_static_node_init();
        let struct_init = self.generate_static_struct_init();

        // For compile-time graphs, generate a static process() method
        let process_method = self.generate_static_process()?;
        let get_stream_output_method = self.generate_static_get_stream_output();
        let clear_event_outputs_method = self.generate_static_clear_event_outputs();
        let process_event_inputs_method = self.generate_static_process_event_inputs();
        let event_handler_methods = self.generate_static_event_handler_methods();

        // Generate init() calls for each node (handling arrays)
        let node_init_calls: Vec<_> = self.nodes.iter().map(|node| {
            let name = &node.name;
            if node.array_size.is_some() {
                // Array: iterate and init each element
                quote! {
                    for node in self.#name.iter_mut() {
                        node.init(sample_rate);
                    }
                }
            } else {
                // Single node: init directly
                quote! {
                    self.#name.init(sample_rate);
                }
            }
        }).collect();

        // Generate NIH-plug params struct if nih_params flag is set
        let nih_params_output = if self.nih_params {
            self.generate_nih_params_struct(name)
        } else {
            quote! {}
        };

        Ok(quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #name {
                #(#fields),*
            }

            impl #name {
                #[allow(unused_variables, unused_mut)]
                pub fn new() -> Self {
                    let sample_rate = 44100.0; // Default sample rate, will be set via init()

                    // Initialize input parameters
                    #(#input_params)*

                    // Initialize output parameters
                    #(#output_params)*

                    // Initialize nodes (direct instantiation)
                    #(#node_init)*

                    Self {
                        #struct_init
                    }
                }

                #process_method

                #get_stream_output_method

                #clear_event_outputs_method

                #process_event_inputs_method

                #(#event_handler_methods)*
            }

            // Generate SignalProcessor implementation for compile-time graphs
            impl ::oscen::SignalProcessor for #name {
                fn init(&mut self, sample_rate: f32) {
                    self.sample_rate = sample_rate;
                    // Call init() on all child nodes
                    #(#node_init_calls)*
                }

                fn process(&mut self) {
                    // This is already implemented in the impl block above
                }
            }

            #nih_params_output
        })
    }
}

// Add Clone impls for AST types
impl Clone for InputDecl {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.clone(),
            ty: self.ty.clone(),
            default: self.default.clone(),
            spec: self.spec.clone(),
        }
    }
}

impl Clone for OutputDecl {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.clone(),
            ty: self.ty.clone(),
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
