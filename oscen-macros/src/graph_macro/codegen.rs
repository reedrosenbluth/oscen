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

        // Infer node endpoint types from connections
        // This builds a registry so we don't need string matching heuristics
        self.infer_node_endpoint_types(&mut type_ctx);

        // Validate each connection
        for conn in &self.connections {
            // Validate source and destination independently
            type_ctx.validate_source(&conn.source)?;
            type_ctx.validate_destination(&conn.dest)?;

            // Validate type compatibility
            type_ctx.validate_connection(&conn.source, &conn.dest)?;

            // Validate that EVENT node endpoints have resolved types (CMajor-style requirement)
            // This is CRITICAL for static graphs since event endpoints need storage generation
            // Stream and Value endpoints can be inferred or left untyped - they don't need special storage
            if let Some(node_name) = Self::extract_root_node(&conn.source) {
                if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.source) {
                    if let Some(endpoint_type) = type_ctx.get_node_endpoint_type(&node_name.to_string(), &endpoint_name.to_string()) {
                        // Only validate event endpoints
                        if endpoint_type == EndpointKind::Event {
                            // Event endpoint has a type, this is good!
                        }
                    } else {
                        // No type could be inferred - check if it might be an event by checking the connection chain
                        // If the destination is an event, then the source must also be an event
                        if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                            if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                                if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(&dest_node.to_string(), &dest_endpoint.to_string()) {
                                    return Err(syn::Error::new(
                                        proc_macro2::Span::call_site(),
                                        format!(
                                            "Cannot infer type for event endpoint {}.{}. Event endpoints must trace back to a graph input with explicit type (e.g., 'input midi_in: event'). Consider adding an event input to the graph.",
                                            node_name, endpoint_name
                                        )
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            if let Some(node_name) = Self::extract_root_node(&conn.dest) {
                if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                    if let Some(endpoint_type) = type_ctx.get_node_endpoint_type(&node_name.to_string(), &endpoint_name.to_string()) {
                        // Only validate event endpoints
                        if endpoint_type == EndpointKind::Event {
                            // Event endpoint has a type, this is good!
                        }
                    } else {
                        // No type could be inferred - check if it might be an event by checking the connection chain
                        // If the source is an event, then the destination must also be an event
                        if let Some(src_node) = Self::extract_root_node(&conn.source) {
                            if let Some(src_endpoint) = Self::extract_endpoint_field(&conn.source) {
                                if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(&src_node.to_string(), &src_endpoint.to_string()) {
                                    return Err(syn::Error::new(
                                        proc_macro2::Span::call_site(),
                                        format!(
                                            "Cannot infer type for event endpoint {}.{}. Event endpoints must trace back to a graph input/output with explicit type (e.g., 'input midi_in: event' or 'output events_out: event').",
                                            node_name, endpoint_name
                                        )
                                    ));
                                }
                            }
                        }
                    }
                }
            }
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
                // Special handling for ArrayEventOutput markers (like .voices)
                // These connections indicate event routing
                if let Some(source_node) = Self::extract_root_node(&conn.source) {
                    if let Some(source_endpoint) = Self::extract_endpoint_field(&conn.source) {
                        if source_endpoint == "voices" {
                            // This is an ArrayEventOutput marker
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

    /// Generate event routing from graph inputs/nodes to node storage
    /// This copies events into the node's event storage fields before processing
    fn generate_event_routing(&self) -> Vec<TokenStream> {
        let mut routing = Vec::new();

        // Build type context to check endpoint types
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        for conn in &self.connections {
            // Check if destination is a node event endpoint
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                    // Check if this is actually an EVENT endpoint
                    if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                        &dest_node.to_string(),
                        &dest_endpoint.to_string()
                    ) {
                        let dest_array_size = self.get_node_array_size(&dest_node);
                        let storage_field = syn::Ident::new(
                            &format!("{}_{}_events", dest_node, dest_endpoint),
                            dest_node.span()
                        );

                        // Check if source is a graph input
                        if let ConnectionExpr::Ident(source_ident) = &conn.source {
                            if let Some(size) = dest_array_size {
                                // Array destination - clear all queues
                                routing.push(quote! {
                                    for i in 0..#size {
                                        self.#storage_field[i].clear();
                                    }
                                    for event in &self.#source_ident {
                                        let _ = self.#storage_field[0].try_push(event.clone());
                                    }
                                });
                            } else {
                                // Single destination - copy events
                                routing.push(quote! {
                                    self.#storage_field.clear();
                                    for event in &self.#source_ident {
                                        let _ = self.#storage_field.try_push(event.clone());
                                    }
                                });
                            }
                        }
                        // If source is another node, events are handled after that node processes
                        // (we'll handle node-to-node routing separately)
                    }
                }
            }
        }

        routing
    }

    /// Get the output index for a named endpoint on a node
    /// The output index is the position among ALL outputs (value, event, stream)
    /// This matches the order in ProcessingNode::ENDPOINT_DESCRIPTORS
    fn get_node_output_index(&self, _node_name: &str, endpoint_name: &str) -> Option<usize> {
        // For nodes defined in the graph, we'd need type information
        // For now, we'll use a heuristic based on common patterns
        // TODO: Parse endpoint descriptors from actual node types

        // Special cases for known node types
        if endpoint_name == "gate" {
            // MidiVoiceHandler: frequency (0), gate (1)
            return Some(1);
        }
        if endpoint_name == "note_on" {
            // MidiParser: note_on (0), note_off (1)
            return Some(0);
        }
        if endpoint_name == "note_off" {
            // MidiParser: note_on (0), note_off (1)
            return Some(1);
        }

        // Default: first output = index 0
        Some(0)
    }

    /// Generate event routing from pending_events (StaticContext) to destination storage
    /// This drains events emitted during event handlers and process() and routes them
    /// to their destinations, supporting both scalar and array routing.
    fn generate_pending_event_routing(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut routing = Vec::new();

        // Build type context to identify event endpoints
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Find all event OUTPUT connections from this node
        for conn in &self.connections {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if source_node == node_name {
                    if let Some(source_endpoint) = Self::extract_endpoint_field(&conn.source) {
                        // Skip ArrayEventOutput markers - those are handled by generate_array_event_routing()
                        if source_endpoint == "voices" {
                            continue;
                        }

                        // Check if this is an event endpoint
                        if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                            &source_node.to_string(),
                            &source_endpoint.to_string()
                        ) {
                            // Determine the output index for this endpoint
                            let output_index = self.get_node_output_index(
                                &source_node.to_string(),
                                &source_endpoint.to_string()
                            ).unwrap_or(0);

                            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                                if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                                    let dest_storage = syn::Ident::new(
                                        &format!("{}_{}_events", dest_node, dest_endpoint),
                                        dest_node.span()
                                    );

                                    // Check if both source and destination are arrays
                                    // If so, skip - this will be handled by array-to-array routing
                                    let source_is_array = self.get_node_array_size(&source_node).is_some();
                                    let dest_is_array = self.get_node_array_size(&dest_node).is_some();

                                    if source_is_array && dest_is_array {
                                        // Skip array-to-array connections - handled by generate_array_to_array_event_routing()
                                        continue;
                                    }

                                    // Check if destination is an array
                                    if let Some(dest_array_size) = self.get_node_array_size(&dest_node) {
                                        // Array destination: route based on array_index in PendingEvent
                                        routing.push(quote! {
                                            // Route events from pending_events to array destinations
                                            for pending in pending_events.iter() {
                                                if pending.output_index == #output_index {
                                                    if let Some(array_idx) = pending.array_index {
                                                        if array_idx < #dest_array_size {
                                                            let _ = self.#dest_storage[array_idx].try_push(pending.event.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    } else {
                                        // Scalar destination: route all events with this output_index
                                        routing.push(quote! {
                                            // Route events from pending_events to scalar destination
                                            for pending in pending_events.iter() {
                                                if pending.output_index == #output_index && pending.array_index.is_none() {
                                                    let _ = self.#dest_storage.try_push(pending.event.clone());
                                                }
                                            }
                                        });
                                    }
                                }
                            } else if self.outputs.iter().any(|o| o.name == *Self::extract_root_node(&conn.dest).unwrap()) {
                                // Destination is a graph output
                                let dest_ident = Self::extract_root_node(&conn.dest).unwrap();
                                routing.push(quote! {
                                    // Route events to graph output
                                    for pending in pending_events.iter() {
                                        if pending.output_index == #output_index && pending.array_index.is_none() {
                                            self.#dest_ident.push(pending.event.clone());
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
            }
        }

        // Clear pending_events after routing
        if !routing.is_empty() {
            routing.push(quote! {
                pending_events.clear();
            });
        }

        routing
    }

    /// Generate cleanup code to clear array event input storage after nodes have processed
    fn generate_array_event_input_cleanup(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut cleanup = Vec::new();

        // Check if this is an array node
        let array_size = match self.get_node_array_size(node_name) {
            Some(size) => size,
            None => return cleanup,
        };

        // Build type context
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Find event input connections to this array node
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                        // Check if this is an event endpoint
                        if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                            &dest_node.to_string(),
                            &dest_endpoint.to_string()
                        ) {
                            // Check if source is also an array (array-to-array connection)
                            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                                if self.get_node_array_size(&source_node).is_some() {
                                    // This is an array-to-array event connection
                                    // Clear the destination storage after processing
                                    let dest_storage = syn::Ident::new(
                                        &format!("{}_{}_events", dest_node, dest_endpoint),
                                        dest_node.span()
                                    );

                                    cleanup.push(quote! {
                                        for i in 0..#array_size {
                                            self.#dest_storage[i].clear();
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        cleanup
    }

    /// Generate array-to-array event routing
    /// Handles connections like: array_source[i].event_out -> array_dest[i].event_in
    /// by copying from source output storage to destination input storage
    fn generate_array_to_array_event_routing(&self, source_node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut routing = Vec::new();

        // Build type context
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Check if source is an array node
        let source_array_size = match self.get_node_array_size(source_node_name) {
            Some(size) => size,
            None => return routing,
        };

        // Find event output connections from this array node
        for conn in &self.connections {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if source_node == source_node_name {
                    if let Some(source_endpoint) = Self::extract_endpoint_field(&conn.source) {
                        // Skip .voices (ArrayEventOutput marker)
                        if source_endpoint == "voices" {
                            continue;
                        }

                        // Check if this is an event endpoint
                        if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                            &source_node.to_string(),
                            &source_endpoint.to_string()
                        ) {
                            // Check if destination is also an array node
                            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                                if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                                    if let Some(dest_array_size) = self.get_node_array_size(&dest_node) {
                                        // Array-to-array event connection
                                        let source_storage = syn::Ident::new(
                                            &format!("{}_{}_events", source_node, source_endpoint),
                                            source_node.span()
                                        );
                                        let dest_storage = syn::Ident::new(
                                            &format!("{}_{}_events", dest_node, dest_endpoint),
                                            dest_node.span()
                                        );

                                        let size = std::cmp::min(source_array_size, dest_array_size);

                                        routing.push(quote! {
                                            // Copy events from source[i] to dest[i], then clear source
                                            for i in 0..#size {
                                                // Don't clear dest here - it will be cleared after processing
                                                for event in &self.#source_storage[i] {
                                                    let _ = self.#dest_storage[i].try_push(event.clone());
                                                }
                                                self.#source_storage[i].clear();
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        routing
    }

    /// Generate array event routing for nodes that implement ArrayEventOutput
    /// Detects connections like: voice_allocator.voices -> voice_handlers.note_on
    /// where .voices is a field of type [EventOutput; N] indicating ArrayEventOutput
    fn generate_array_event_routing(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut routing = Vec::new();

        // Build type context
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Look for connections FROM this node where the source endpoint might be an array output marker
        // Pattern: node.voices -> array_dest.endpoint
        // where "voices" is a field of type [EventOutput; N]
        for conn in &self.connections {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if source_node == node_name {
                    if let Some(source_endpoint) = Self::extract_endpoint_field(&conn.source) {
                        // Check if this is an ArrayEventOutput marker (like "voices")
                        // by checking if the endpoint name is "voices" (convention)
                        // TODO: Make this more robust by checking field type
                        if source_endpoint == "voices" {
                            // Check if destination is an array node
                            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                                if let Some(dest_endpoint) = Self::extract_endpoint_field(&conn.dest) {
                                    if let Some(dest_array_size) = self.get_node_array_size(&dest_node) {
                                        // Map destination endpoint back to source input endpoint
                                        // voice_allocator.voices -> voice_handlers.note_on
                                        // means: route events from voice_allocator.note_on (input)
                                        //        to voice_handlers[i].note_on (array input)
                                        let source_input_storage = syn::Ident::new(
                                            &format!("{}_{}_events", source_node, dest_endpoint),
                                            source_node.span()
                                        );
                                        let dest_storage = syn::Ident::new(
                                            &format!("{}_{}_events", dest_node, dest_endpoint),
                                            dest_node.span()
                                        );

                                        // Map endpoint name to input index
                                        // For VoiceAllocator: note_on = 0, note_off = 1
                                        let input_index = if dest_endpoint == "note_on" {
                                            0usize
                                        } else if dest_endpoint == "note_off" {
                                            1usize
                                        } else {
                                            0usize // Default
                                        };

                                        routing.push(quote! {
                                            // Clear all destination queues
                                            for i in 0..#dest_array_size {
                                                self.#dest_storage[i].clear();
                                            }
                                            // Route events through the ArrayEventOutput node
                                            for event in &self.#source_input_storage {
                                                if let Some(voice_idx) = self.#source_node.route_event(#input_index, event) {
                                                    if voice_idx < #dest_array_size {
                                                        let _ = self.#dest_storage[voice_idx].try_push(event.clone());
                                                    }
                                                }
                                            }
                                            // Clear source storage to prevent re-routing same events next frame
                                            self.#source_input_storage.clear();
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        routing
    }

    /// Generate event processing for array nodes - processes each element individually
    /// and routes events to the correct output storage based on array index
    fn generate_array_event_handlers(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut handlers = Vec::new();

        let array_size = match self.get_node_array_size(node_name) {
            Some(size) => size,
            None => return handlers,
        };

        // Build type context
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Find event inputs for this node
        let mut event_inputs = Vec::new();
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                        if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                            &dest_node.to_string(),
                            &endpoint_name.to_string()
                        ) {
                            if !event_inputs.contains(&endpoint_name.to_string()) {
                                event_inputs.push(endpoint_name.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Find event outputs for this node
        let mut event_outputs = Vec::new();
        for conn in &self.connections {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if source_node == node_name {
                    if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.source) {
                        if endpoint_name != "voices" { // Skip ArrayEventOutput markers
                            if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                                &source_node.to_string(),
                                &endpoint_name.to_string()
                            ) {
                                if !event_outputs.contains(&endpoint_name.to_string()) {
                                    event_outputs.push(endpoint_name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Generate event handler calls for each input
        let mut handler_calls = Vec::new();
        for input_name in &event_inputs {
            let storage_field = syn::Ident::new(
                &format!("{}_{}_events", node_name, input_name),
                node_name.span()
            );
            let handle_method = syn::Ident::new(
                &format!("handle_{}_events", input_name),
                node_name.span()
            );
            handler_calls.push(quote! {
                self.#node_name[array_idx].#handle_method(&self.#storage_field[array_idx], &mut static_ctx);
            });
        }

        // Generate event routing for each output
        let mut routing_calls = Vec::new();
        for output_name in &event_outputs {
            let output_storage = syn::Ident::new(
                &format!("{}_{}_events", node_name, output_name),
                node_name.span()
            );
            let output_index = self.get_node_output_index(
                &node_name.to_string(),
                output_name
            ).unwrap_or(0);
            routing_calls.push(quote! {
                for pending in pending_events.iter() {
                    if pending.output_index == #output_index && pending.array_index.is_none() {
                        let _ = self.#output_storage[array_idx].try_push(pending.event.clone());
                    }
                }
            });
        }

        // Generate processing for each array element
        handlers.push(quote! {
            for array_idx in 0..#array_size {
                let mut static_ctx = ::oscen::graph::StaticContext::new(&mut pending_events);

                // Call event handlers for this array element
                #(#handler_calls)*

                // Process the node
                self.#node_name[array_idx].process();

                // Route emitted events to this node's output storage
                #(#routing_calls)*

                pending_events.clear();
            }
        });

        handlers
    }

    /// Generate event handler calls for a node
    /// This calls the node's handle_{endpoint}_events() methods with events from the graph storage
    fn generate_node_event_handlers(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut handlers = Vec::new();

        // Build type context to check endpoint types
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Check if this node uses ArrayEventOutput routing (e.g., has .voices connections)
        // If so, skip generating event handler calls because routing is handled by route_event()
        let uses_array_output = self.connections.iter().any(|conn| {
            if let Some(source_node) = Self::extract_root_node(&conn.source) {
                if source_node == node_name {
                    if let Some(endpoint) = Self::extract_endpoint_field(&conn.source) {
                        return endpoint == "voices";
                    }
                }
            }
            false
        });

        if uses_array_output {
            // Skip event handler calls for ArrayEventOutput nodes
            return handlers;
        }

        // Check if this is an array node
        let array_size = self.get_node_array_size(node_name);

        // Find all event input connections for this node
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                        // Check if this is actually an EVENT endpoint using type context
                        if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                            &dest_node.to_string(),
                            &endpoint_name.to_string()
                        ) {
                            let storage_field = syn::Ident::new(
                                &format!("{}_{}_events", node_name, endpoint_name),
                                node_name.span()
                            );
                            let handle_method = syn::Ident::new(
                                &format!("handle_{}_events", endpoint_name),
                                node_name.span()
                            );

                            if let Some(_size) = array_size {
                                // Array node: call handler for each element
                                // Note: For arrays, event handlers are not called here
                                // Instead, they're called individually in generate_static_process
                                // to properly track array indices for event routing
                            } else {
                                // Single node: call handler once
                                handlers.push(quote! {
                                    self.#node_name.#handle_method(&self.#storage_field, &mut static_ctx);
                                });
                            }
                        }
                    }
                }
            }
        }

        handlers
    }

    /// Collect all node event endpoints from connections using type information
    /// Returns: Vec<(node_name, endpoint_name, is_input)>
    fn collect_node_event_endpoints(&self) -> Vec<(syn::Ident, String, bool, Option<usize>)> {
        use std::collections::HashSet;
        let mut event_endpoints = Vec::new();
        let mut seen = HashSet::new();

        // Build type context to get endpoint types
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Now iterate through connections and collect event endpoints
        for conn in &self.connections {
            // Check source (output endpoint)
            if let Some(node_name) = Self::extract_root_node(&conn.source) {
                if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.source) {
                    // Look up the actual type from type context
                    if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                        &node_name.to_string(),
                        &endpoint_name.to_string()
                    ) {
                        let array_size = self.get_node_array_size(&node_name);
                        let key = (node_name.to_string(), endpoint_name.to_string(), false);
                        if seen.insert(key.clone()) {
                            event_endpoints.push((node_name.clone(), endpoint_name.to_string(), false, array_size));
                        }
                    }
                }
            }

            // Check destination (input endpoint)
            if let Some(node_name) = Self::extract_root_node(&conn.dest) {
                if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                    // Look up the actual type from type context
                    if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                        &node_name.to_string(),
                        &endpoint_name.to_string()
                    ) {
                        let array_size = self.get_node_array_size(&node_name);
                        let key = (node_name.to_string(), endpoint_name.to_string(), true);
                        if seen.insert(key.clone()) {
                            event_endpoints.push((node_name.clone(), endpoint_name.to_string(), true, array_size));
                        }
                    }
                }
            }
        }

        event_endpoints
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

        // Add node event storage fields
        let node_event_fields = self.collect_node_event_endpoints();
        for (node_name, endpoint_name, _is_input, array_size) in &node_event_fields {
            let storage_field = syn::Ident::new(
                &format!("{}_{}_events", node_name, endpoint_name),
                node_name.span()
            );
            if let Some(size) = array_size {
                // Array node: initialize array of queues
                fields.push(quote! {
                    #storage_field: [(); #size].map(|_| ::oscen::graph::StaticEventQueue::new())
                });
            } else {
                // Single node: initialize with empty queue
                fields.push(quote! { #storage_field: ::oscen::graph::StaticEventQueue::new() });
            }
        }

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

    fn get_node_array_size(&self, name: &syn::Ident) -> Option<usize> {
        self.nodes
            .iter()
            .find(|n| n.name == *name)
            .and_then(|n| n.array_size)
    }

    /// Generate connection assignments for a specific node
    /// Returns assignments that should be executed before processing this node
    fn generate_connection_assignments_for_node(&self, node_name: &syn::Ident) -> Vec<TokenStream> {
        let mut assignments = Vec::new();

        // Build type context to filter out event connections
        let mut type_ctx = TypeContext::new();
        for input in &self.inputs {
            type_ctx.register_input(&input.name, input.kind);
        }
        for output in &self.outputs {
            type_ctx.register_output(&output.name, output.kind);
        }
        self.infer_node_endpoint_types(&mut type_ctx);

        // Find all connections where this node is the destination
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    // This connection feeds into the current node
                    if let Some(source_node) = Self::extract_root_node(&conn.source) {
                        let source_field = Self::extract_endpoint_field(&conn.source);

                        if let Some(dest_field) = Self::extract_endpoint_field(&conn.dest) {
                            // Skip event connections - they're handled separately
                            // Check both destination and source to catch event connections even when types are unknown
                            let dest_is_event = matches!(
                                type_ctx.get_node_endpoint_type(&dest_node.to_string(), &dest_field.to_string()),
                                Some(EndpointKind::Event)
                            );

                            let source_is_event = if let Some(source_field) = source_field {
                                matches!(
                                    type_ctx.get_node_endpoint_type(&source_node.to_string(), &source_field.to_string()),
                                    Some(EndpointKind::Event)
                                )
                            } else {
                                false
                            };

                            if dest_is_event || source_is_event {
                                continue;
                            }

                            // Skip ArrayEventOutput marker connections (like .voices)
                            // These are handled by generate_array_event_routing()
                            if let Some(ref field) = source_field {
                                if *field == "voices" {
                                    continue;
                                }
                            }

                            let dest_array_size = self.get_node_array_size(dest_node);
                            let source_array_size = self.get_node_array_size(source_node);

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
                                                    &self.#source_node[i] #source_access,
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
                                                    &self.#source_node[i] #source_access,
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
                                                &self.#source_node #source_access,
                                                &mut self.#dest_node[i].#dest_field
                                            );
                                        }
                                    });
                                }
                                (None, Some(_)) => {
                                    // Array-to-Scalar reduction (Summing)
                                    // Note: source_access must be present for nodes, but maybe not?
                                    // If source is array node, we iterate.
                                    if let Some(field) = source_field {
                                        assignments.push(quote! {
                                            self.#dest_node.#dest_field = self.#source_node.iter().map(|n| n.#field).sum();
                                        });
                                    } else {
                                        // Array of scalars? Not supported by current Node definition
                                        // But if it were, it would be:
                                        assignments.push(quote! {
                                            self.#dest_node.#dest_field = self.#source_node.iter().sum();
                                        });
                                    }
                                }
                                (None, None) => {
                                    // Scalar-to-Scalar using trait dispatch
                                    assignments.push(quote! {
                                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                            &self.#source_node #source_access,
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
    fn generate_static_process(&self) -> Result<TokenStream> {
        let sorted_nodes = self.topological_sort_nodes()?;

        // Generate process calls for each node in topological order
        let mut process_body = Vec::new();

        // First, route events from graph inputs to node storages
        let event_routing = self.generate_event_routing();
        process_body.extend(event_routing);

        for node_name in &sorted_nodes {
            // First, generate connection assignments that feed into this node
            let assignments = self.generate_connection_assignments_for_node(node_name);
            process_body.extend(assignments);

            // Handle events and process the node in a scope with its own static_ctx
            let event_handlers = self.generate_node_event_handlers(node_name);
            let process_call = if let Some(array_size) = self.get_node_array_size(node_name) {
                quote! {
                    for i in 0..#array_size {
                        self.#node_name[i].process();
                    }
                }
            } else {
                quote! {
                    self.#node_name.process();
                }
            };

            // Special handling for array nodes with event inputs/outputs
            if let Some(_array_size) = self.get_node_array_size(node_name) {
                // Check if this array node has event inputs that need handlers
                let has_event_inputs = self.connections.iter().any(|conn| {
                    if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                        if dest_node == node_name {
                            if let Some(endpoint_name) = Self::extract_endpoint_field(&conn.dest) {
                                // Build type context to check
                                let mut type_ctx = TypeContext::new();
                                for input in &self.inputs {
                                    type_ctx.register_input(&input.name, input.kind);
                                }
                                for output in &self.outputs {
                                    type_ctx.register_output(&output.name, output.kind);
                                }
                                self.infer_node_endpoint_types(&mut type_ctx);

                                if let Some(EndpointKind::Event) = type_ctx.get_node_endpoint_type(
                                    &dest_node.to_string(),
                                    &endpoint_name.to_string()
                                ) {
                                    return true;
                                }
                            }
                        }
                    }
                    false
                });

                if has_event_inputs {
                    // Generate individual processing for each array element
                    let array_handlers = self.generate_array_event_handlers(node_name);
                    process_body.push(quote! {
                        {
                            #(#array_handlers)*
                        }
                    });
                } else {
                    process_body.push(quote! {
                        {
                            #process_call
                        }
                    });
                }
            } else if !event_handlers.is_empty() {
                // Scalar node with event handlers
                process_body.push(quote! {
                    {
                        let mut static_ctx = ::oscen::graph::StaticContext::new(&mut pending_events);
                        #(#event_handlers)*
                        #process_call
                    }
                });
            } else {
                // Scalar node without event handlers
                process_body.push(quote! {
                    {
                        #process_call
                    }
                });
            }

            // Route pending events from StaticContext to their destinations
            let pending_event_routing = self.generate_pending_event_routing(node_name);
            process_body.extend(pending_event_routing);

            // After processing, handle array event routing (CMajor-style multiplexing)
            // This routes events from nodes that implement ArrayEventOutput to array destinations
            let array_routing = self.generate_array_event_routing(node_name);
            process_body.extend(array_routing);

            // Generate array-to-array event routing (like voice_handlers.gate -> voices.gate)
            let array_event_copy = self.generate_array_to_array_event_routing(node_name);
            process_body.extend(array_event_copy);

            // Clear array event input storage after processing
            let array_cleanup = self.generate_array_event_input_cleanup(node_name);
            process_body.extend(array_cleanup);
        }

        // Generate assignments for connections to outputs
        for conn in &self.connections {
            if let Some(dest_ident) = Self::extract_root_node(&conn.dest) {
                // Check if destination is an output
                if let Some(output_decl) = self.outputs.iter().find(|o| o.name == *dest_ident) {
                    // This connection targets an output - generate assignment/copy based on kind
                    if let Some(source_node) = Self::extract_root_node(&conn.source) {
                        if let Some(source_field) = Self::extract_endpoint_field(&conn.source) {
                            match output_decl.kind {
                                EndpointKind::Stream | EndpointKind::Value => {
                                    if let Some(_src_array_size) = self.get_node_array_size(source_node) {
                                        // Array-to-Output: Summing
                                        process_body.push(quote! {
                                            self.#dest_ident = self.#source_node.iter().map(|n| n.#source_field).sum();
                                        });
                                    } else {
                                        // Scalar-to-Output
                                        process_body.push(quote! {
                                            self.#dest_ident = self.#source_node.#source_field;
                                        });
                                    }
                                }
                                EndpointKind::Event => {
                                    let storage_field = syn::Ident::new(
                                        &format!("{}_{}_events", source_node, source_field),
                                        source_node.span(),
                                    );
                                    if let Some(array_size) = self.get_node_array_size(source_node) {
                                        // Array event source: copy all events from each element
                                        process_body.push(quote! {
                                            self.#dest_ident.clear();
                                            for i in 0..#array_size {
                                                for event in &self.#storage_field[i] {
                                                    let _ = self.#dest_ident.try_push(event.clone());
                                                }
                                            }
                                        });
                                    } else {
                                        // Scalar event source
                                        process_body.push(quote! {
                                            self.#dest_ident.clear();
                                            for event in &self.#storage_field {
                                                let _ = self.#dest_ident.try_push(event.clone());
                                            }
                                        });
                                    }
                                }
                            }
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

        // Match dynamic graph API: process() with no return value
        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) {
                use ::oscen::SignalProcessor as _;
                use ::oscen::graph::ArrayEventOutput as _;

                // Create shared pending events buffer for StaticContext
                // StaticContext will be created in scopes as needed to avoid borrow conflicts
                let mut pending_events = ::arrayvec::ArrayVec::<
                    ::oscen::graph::static_context::PendingEvent,
                    64
                >::new();

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
                        _ctx: &mut ::oscen::graph::StaticContext
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

        // Add event storage fields for node event endpoints
        // Following CMajor's approach: embed event arrays in the graph state struct
        // This allows nodes to remain unchanged while the graph manages event routing
        let node_event_fields = self.collect_node_event_endpoints();
        for (node_name, endpoint_name, _is_input, array_size) in &node_event_fields {
            let storage_field = syn::Ident::new(
                &format!("{}_{}_events", node_name, endpoint_name),
                node_name.span()
            );
            if let Some(size) = array_size {
                // Array node: generate [StaticEventQueue; N]
                fields.push(quote! { pub #storage_field: [::oscen::graph::StaticEventQueue; #size] });
            } else {
                // Single node: generate StaticEventQueue
                fields.push(quote! { pub #storage_field: ::oscen::graph::StaticEventQueue });
            }
        }

        let input_params = self.generate_static_input_params();
        let output_params = self.generate_static_output_params();
        let node_init = self.generate_static_node_init();
        let struct_init = self.generate_static_struct_init();

        // For compile-time graphs, generate a static process() method
        let process_method = self.generate_static_process()?;
        let get_stream_output_method = self.generate_static_get_stream_output();
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
            spec: None, // Skip spec for now
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
