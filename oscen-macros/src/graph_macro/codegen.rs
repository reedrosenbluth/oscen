use super::ast::*;
use super::type_check::TypeContext;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, Result};

pub fn generate(graph_def: &GraphDef) -> Result<TokenStream> {
    let mut ctx = CodegenContext::new(graph_def.compile_time);

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
    compile_time: bool,
    inputs: Vec<InputDecl>,
    outputs: Vec<OutputDecl>,
    nodes: Vec<NodeDecl>,
    connections: Vec<ConnectionStmt>,
}

impl CodegenContext {
    fn new(compile_time: bool) -> Self {
        Self {
            compile_time,
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
        }
    }

    fn normalize_constructor(expr: &Expr) -> TokenStream {
        match expr {
            Expr::Path(path) => quote! { #path::new(sample_rate) },
            _ => quote! { #expr },
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
                    // Runtime graph outputs are inputs TO the output capture
                    quote! {
                        let #name = {
                            let key = graph.allocate_endpoint(::oscen::graph::EndpointType::Stream);
                            let endpoint = ::oscen::InputEndpoint::new(key);
                            ::oscen::StreamInput::new(endpoint)
                        };
                    }
                }
                EndpointKind::Value => {
                    quote! {
                        let #name = {
                            let key = graph.allocate_endpoint(::oscen::graph::EndpointType::Value);
                            let endpoint = ::oscen::InputEndpoint::new(key);
                            ::oscen::ValueInput::new(endpoint)
                        };
                    }
                }
                EndpointKind::Event => {
                    quote! {
                        let #name = {
                            let key = graph.allocate_endpoint(::oscen::graph::EndpointType::Event);
                            let endpoint = ::oscen::InputEndpoint::new(key);
                            ::oscen::EventInput::new(endpoint)
                        };
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
                let constructor = Self::normalize_constructor(&node.constructor);

                if let Some(_array_size) = node.array_size {
                    // Use add_node_array for runtime graphs
                    let array_id = name.to_string();
                    vec![quote! {
                        let #name = graph.add_node_array(
                            #array_id,
                            || #constructor
                        );
                    }]
                } else {
                    // Single instance
                    vec![quote! {
                        let #name = graph.add_node(#constructor);
                    }]
                }
            })
            .collect()
    }

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

    fn generate_connections(&self) -> Result<Vec<TokenStream>> {
        if self.connections.is_empty() {
            return Ok(vec![]);
        }

        let mut temp_stmts = Vec::new(); // Temporary variable declarations
        let mut regular_connections = Vec::new(); // Connection expressions
        let mut output_assignments = Vec::new();
        let mut temp_counter = 0;

        for conn in &self.connections {
            // Check if destination is an output (only for static graphs)
            if self.compile_time {
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
    /// 1. Broadcast marker: `voice_allocator.voice() -> voice_handlers.note_on()`
    /// 2. Array-to-array: `voice_handlers.frequency() -> voices.frequency()`
    /// 3. Scalar-to-array: `cutoff -> voices.cutoff()`
    /// 4. Array-to-single: `voices.output() -> tremolo.input()` (automatic sum/mix)
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
                    // Pattern 1: Broadcast marker (e.g., voice())
                    if let ConnectionExpr::Method(src_obj, src_method, _src_args) = source {
                        if let ConnectionExpr::Ident(src_base) = &**src_obj {
                            if src_method == "voice" {
                                // Generate N connections: src.voice(i) -> dest[i].method()
                                let mut connections = Vec::new();
                                for i in 0..dest_array_size {
                                    let src_indexed = quote! { #src_base.voice(#i) };

                                    let dest_access = if self.compile_time {
                                        // Static: voice_handlers_0
                                        let dest_indexed_name = syn::Ident::new(
                                            &format!("{}_{}", dest_base, i),
                                            dest_base.span(),
                                        );
                                        quote! { #dest_indexed_name }
                                    } else {
                                        // Runtime: voice_handlers[0]
                                        quote! { #dest_base[#i] }
                                    };

                                    let dest_call = if dest_args.is_empty() {
                                        quote! { #dest_access.#dest_method }
                                    } else {
                                        quote! { #dest_access.#dest_method(#(#dest_args),*) }
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
                                        // Different syntax for static vs runtime graphs
                                        let (src_access, dest_access) = if self.compile_time {
                                            // Static: voice_handlers_0
                                            let src_indexed_name = syn::Ident::new(
                                                &format!("{}_{}", src_base, i),
                                                src_base.span(),
                                            );
                                            let dest_indexed_name = syn::Ident::new(
                                                &format!("{}_{}", dest_base, i),
                                                dest_base.span(),
                                            );
                                            (quote! { #src_indexed_name }, quote! { #dest_indexed_name })
                                        } else {
                                            // Runtime: voice_handlers[0]
                                            (quote! { #src_base[#i] }, quote! { #dest_base[#i] })
                                        };

                                        let src_call = if src_args.is_empty() {
                                            quote! { #src_access.#src_method }
                                        } else {
                                            quote! { #src_access.#src_method(#(#src_args),*) }
                                        };

                                        let dest_call = if dest_args.is_empty() {
                                            quote! { #dest_access.#dest_method }
                                        } else {
                                            quote! { #dest_access.#dest_method(#(#dest_args),*) }
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
                                let dest_access = if self.compile_time {
                                    // Static: voice_handlers_0
                                    let dest_indexed_name = syn::Ident::new(
                                        &format!("{}_{}", dest_base, i),
                                        dest_base.span(),
                                    );
                                    quote! { #dest_indexed_name }
                                } else {
                                    // Runtime: voice_handlers[0]
                                    quote! { #dest_base[#i] }
                                };

                                let dest_call = if dest_args.is_empty() {
                                    quote! { #dest_access.#dest_method }
                                } else {
                                    quote! { #dest_access.#dest_method(#(#dest_args),*) }
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

        // Pattern 4: Array-to-single (e.g., voices.output() -> tremolo.input())
        // All array elements connect to the same single destination (automatic sum/mix)
        if let ConnectionExpr::Method(src_obj, src_method, src_args) = source {
            if let ConnectionExpr::Ident(src_base) = &**src_obj {
                if let Some(src_array_size) = self
                    .nodes
                    .iter()
                    .find(|n| n.name == *src_base)
                    .and_then(|n| n.array_size)
                {
                    // Destination is NOT an array (single input for mixing)
                    let mut connections = Vec::new();
                    for i in 0..src_array_size {
                        let src_access = if self.compile_time {
                            // Static: voices_0
                            let src_indexed_name = syn::Ident::new(
                                &format!("{}_{}", src_base, i),
                                src_base.span(),
                            );
                            quote! { #src_indexed_name }
                        } else {
                            // Runtime: voices[0]
                            quote! { #src_base[#i] }
                        };

                        let src_call = if src_args.is_empty() {
                            quote! { #src_access.#src_method }
                        } else {
                            quote! { #src_access.#src_method(#(#src_args),*) }
                        };

                        let dest_expr = self.generate_connection_expr(dest)?;
                        connections.push(quote! {
                            #src_call >> #dest_expr
                        });
                    }
                    return Ok(Some(connections));
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
            let cache_name = syn::Ident::new(&format!("{}_cache", name), name.span());
            fields.push(quote! { #name });
            fields.push(quote! { #cache_name: 0.0 });
        }

        // Add node handles
        for node in &self.nodes {
            let name = &node.name;
            // For arrays, add_node_array already returns [NodeKey; N], so just use the name
            fields.push(quote! { #name });
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
                self.graph.read_endpoint_value(self.#field_name.key())
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
                        Some(self.graph.read_endpoint_value(self.#field_name.key()))
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
                        Some(::oscen::graph::types::ValueData::scalar(
                            self.graph.read_endpoint_value(self.#field_name.key())
                        ))
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
                fn process(&mut self) {
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

        // Add output capture fields
        // Store both the connection endpoint and cached value
        for output in &self.outputs {
            let field_name = &output.name;
            let cache_name = syn::Ident::new(&format!("{}_cache", field_name), field_name.span());

            let ty = match output.kind {
                EndpointKind::Value => quote! { ::oscen::ValueInput },
                EndpointKind::Event => quote! { ::oscen::EventInput },
                EndpointKind::Stream => quote! { ::oscen::StreamInput },
            };
            fields.push(quote! { #field_name: #ty });
            fields.push(quote! { #cache_name: f32 });
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

        // Collect input/output names for GraphInterface
        // Only collect value inputs for set_input_value (event/stream inputs can't be set)
        let value_input_names: Vec<_> = self.inputs.iter()
            .filter(|i| matches!(i.kind, EndpointKind::Value))
            .map(|i| &i.name)
            .collect();
        let output_names: Vec<_> = self.outputs.iter().map(|o| &o.name).collect();
        let output_cache_names: Vec<_> = self.outputs.iter().map(|o| {
            syn::Ident::new(&format!("{}_cache", o.name), o.name.span())
        }).collect();

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

            // Generate DynNode implementation (required for runtime graphs)
            impl ::oscen::graph::DynNode for #name {}

            // Generate GraphInterface implementation (unified API)
            impl ::oscen::graph::GraphInterface for #name {
                fn process_sample(&mut self) -> f32 {
                    let _ = self.graph.process();

                    // Update output caches by reading from connected endpoints
                    #(self.#output_cache_names = self.graph.read_endpoint_value(self.#output_names.key());)*

                    // Return first output value (or 0.0 if no outputs)
                    #(return self.#output_cache_names;)*
                    0.0
                }

                fn set_input_value(&mut self, name: &str, value: f32) {
                    match name {
                        #(stringify!(#value_input_names) => { self.graph.set_value(&self.#value_input_names, value); },)*
                        _ => {}
                    }
                }

                fn get_output_value(&self, name: &str) -> f32 {
                    match name {
                        #(stringify!(#output_names) => self.#output_cache_names,)*
                        _ => 0.0
                    }
                }

                fn sample_rate(&self) -> f32 {
                    self.graph.sample_rate
                }
            }
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
                EndpointKind::Stream => quote! { ::oscen::StreamInput },
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

        // Collect input/output names for GraphInterface
        // Only collect value inputs for set_input_value (event/stream inputs can't be set as f32)
        let input_names: Vec<_> = self.inputs.iter()
            .filter(|i| matches!(i.kind, EndpointKind::Value))
            .map(|i| &i.name)
            .collect();
        // Only collect stream/value outputs (event outputs are StaticEventQueue, not f32)
        let output_names: Vec<_> = self.outputs.iter()
            .filter(|o| matches!(o.kind, EndpointKind::Stream | EndpointKind::Value))
            .map(|o| &o.name)
            .collect();

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

                    // Create temporary graph for input/output endpoint allocation
                    let mut __temp_graph = ::oscen::Graph::new(sample_rate);

                    // Initialize input parameters (requires graph for endpoint allocation)
                    #(#input_params)*

                    // Initialize output parameters (static values, not endpoint wrappers)
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

            // Generate GraphInterface implementation (unified API)
            impl ::oscen::graph::GraphInterface for #name {
                fn process_sample(&mut self) -> f32 {
                    self.process();
                    // Return first output (or 0.0 if no outputs)
                    if let Some(first_output) = vec![#(self.#output_names),*].first() {
                        return *first_output;
                    }
                    0.0
                }

                fn set_input_value(&mut self, name: &str, value: f32) {
                    match name {
                        #(stringify!(#input_names) => { self.#input_names = value; },)*
                        _ => {}
                    }
                }

                fn get_output_value(&self, name: &str) -> f32 {
                    match name {
                        #(stringify!(#output_names) => self.#output_names,)*
                        _ => 0.0
                    }
                }

                fn sample_rate(&self) -> f32 {
                    self.sample_rate
                }
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
