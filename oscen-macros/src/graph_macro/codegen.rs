use super::ast::*;
use super::rate_analysis::{analyze, EdgeKernel, RateAnalysis};
use super::type_check::TypeContext;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, Result};

/// Field name for the resampler kernel state stored on the graph struct for
/// the connection at `idx` (index into `RateAnalysis::edges`).
fn resampler_field_name(idx: usize) -> syn::Ident {
    syn::Ident::new(&format!("__resampler_{}", idx), proc_macro2::Span::call_site())
}

/// Local-variable name for the upsample buffer associated with edge `idx`.
fn up_buf_name(idx: usize) -> syn::Ident {
    syn::Ident::new(&format!("__up_buf_{}", idx), proc_macro2::Span::call_site())
}

/// Local-variable name for the downsample accumulator buffer associated with
/// edge `idx`.
fn down_buf_name(idx: usize) -> syn::Ident {
    syn::Ident::new(&format!("__down_buf_{}", idx), proc_macro2::Span::call_site())
}

/// Choose the Rust kernel type for an upsampler edge based on policy.
fn kernel_up_type(factor: u32, policy: ConnectionPolicy) -> TokenStream {
    let n = factor as usize;
    match policy {
        ConnectionPolicy::Latch => quote! { ::oscen::resample::LatchUp<#n> },
        ConnectionPolicy::Linear => quote! { ::oscen::resample::LinearUp<#n> },
        ConnectionPolicy::Sinc | ConnectionPolicy::Default => {
            quote! { ::oscen::resample::SincUpFir<#n> }
        }
        ConnectionPolicy::SincIir => quote! { ::oscen::resample::IirHalfbandUp<#n> },
    }
}

/// Choose the Rust kernel type for a downsampler edge based on policy.
fn kernel_down_type(factor: u32, policy: ConnectionPolicy) -> TokenStream {
    let n = factor as usize;
    match policy {
        ConnectionPolicy::Latch => quote! { ::oscen::resample::LatchDown<#n> },
        ConnectionPolicy::Linear => quote! { ::oscen::resample::LinearDown<#n> },
        ConnectionPolicy::Sinc | ConnectionPolicy::Default => {
            quote! { ::oscen::resample::SincDownFir<#n> }
        }
        ConnectionPolicy::SincIir => quote! { ::oscen::resample::IirHalfbandDown<#n> },
    }
}

pub fn generate(graph_def: &GraphDef) -> Result<TokenStream> {
    // Validate rate annotations and edge rate combinations before collecting
    // codegen state so the analysis is available to every emit method.
    let rate_analysis = analyze(graph_def)?;

    let mut ctx = CodegenContext::new(rate_analysis);

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
    /// Rate analysis result. Consumed by emit methods to generate per-edge
    /// resampler fields and (in later tasks) the multi-rate inner loop.
    rate_analysis: RateAnalysis,
}

impl CodegenContext {
    fn new(rate_analysis: RateAnalysis) -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            nih_params: false,
            rate_analysis,
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

    /// Check if an input has a ramp annotation and return the default ramp frames.
    fn is_ramped_input(&self, name: &syn::Ident) -> Option<usize> {
        self.inputs
            .iter()
            .find(|i| i.name == *name && i.kind == EndpointKind::Value)
            .and_then(|i| i.spec.as_ref())
            .and_then(|s| s.ramp)
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
            // Validate destination
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
        self.inputs.iter().flat_map(|input| {
            let name = &input.name;
            let default_val = input.default.as_ref();

            let mut stmts = Vec::new();
            match input.kind {
                EndpointKind::Value => {
                    let default = default_val.map(|d| quote! { #d }).unwrap_or(quote! { 0.0 });
                    if self.is_ramped_input(name).is_some() {
                        stmts.push(quote! {
                            let #name = ::oscen::graph::ValueRampState::new(#default);
                        });
                    } else {
                        stmts.push(quote! {
                            let #name = #default;
                        });
                    }
                }
                EndpointKind::Event => {
                    stmts.push(quote! {
                        let #name = ::oscen::graph::StaticEventQueue::new();
                    });
                }
                EndpointKind::Stream => {
                    stmts.push(quote! {
                        let #name = 0.0f32;
                    });
                    // Block buffer for stream inputs
                    let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                    stmts.push(quote! {
                        let #block_name = [0.0f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE];
                    });
                }
            }
            stmts
        }).collect()
    }

    /// Generate static initialization for output parameters
    /// For static graphs, outputs store actual values (f32) not endpoint wrappers
    fn generate_static_output_params(&self) -> Vec<TokenStream> {
        self.outputs.iter().flat_map(|output| {
            let name = &output.name;
            let mut stmts = Vec::new();

            match output.kind {
                EndpointKind::Stream => {
                    stmts.push(quote! {
                        let #name = 0.0f32;
                    });
                    // Block buffer for stream outputs
                    let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                    stmts.push(quote! {
                        let #block_name = [0.0f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE];
                    });
                }
                EndpointKind::Value => {
                    stmts.push(quote! {
                        let #name = 0.0f32;
                    });
                }
                EndpointKind::Event => {
                    stmts.push(quote! {
                        let #name = ::oscen::graph::StaticEventQueue::new();
                    });
                }
            }
            stmts
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
        let has_ramped = self.has_ramped_inputs();

        let active_ramps_init = if has_ramped {
            quote! { active_ramps: 0, }
        } else {
            quote! {}
        };

        // Add input/output fields (including block buffer fields for streams)
        let input_fields: Vec<_> = self.inputs.iter().flat_map(|input| {
            let name = &input.name;
            let mut fields = vec![quote! { #name }];
            if input.kind == EndpointKind::Stream {
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                fields.push(quote! { #block_name });
            }
            fields
        }).collect();

        let output_fields: Vec<_> = self.outputs.iter().flat_map(|output| {
            let name = &output.name;
            let mut fields = vec![quote! { #name }];
            if output.kind == EndpointKind::Stream {
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                fields.push(quote! { #block_name });
            }
            fields
        }).collect();

        // Add node fields (no IO fields)
        let node_fields: Vec<_> = self.nodes.iter().map(|node| {
            let name = &node.name;
            quote! { #name }
        }).collect();

        // Note: Graph-level event storage is no longer generated
        // Nodes own their own EventInput/EventOutput storage

        quote! {
            sample_rate,
            #active_ramps_init
            #(#input_fields,)*
            #(#output_fields,)*
            #(#node_fields),*
        }
    }

    /// Generate one struct field per cross-rate connection. Each field holds a
    /// resampler kernel instance whose type is chosen by edge direction and
    /// policy. Same-rate edges produce no fields.
    fn generate_resampler_fields(&self) -> Vec<TokenStream> {
        let mut fields = Vec::new();
        for edge in &self.rate_analysis.edges {
            let ty = match edge.kernel {
                EdgeKernel::None => continue,
                EdgeKernel::Up { factor, kind } => kernel_up_type(factor, kind),
                EdgeKernel::Down { factor, kind } => kernel_down_type(factor, kind),
            };
            let field_name = resampler_field_name(edge.edge_index);
            fields.push(quote! { pub #field_name: #ty });
        }
        fields
    }

    /// Generate one initializer per cross-rate connection, calling the kernel
    /// type's `new()` constructor. Order matches `generate_resampler_fields`.
    fn generate_resampler_inits(&self) -> Vec<TokenStream> {
        let mut inits = Vec::new();
        for edge in &self.rate_analysis.edges {
            let ty = match edge.kernel {
                EdgeKernel::None => continue,
                EdgeKernel::Up { factor, kind } => kernel_up_type(factor, kind),
                EdgeKernel::Down { factor, kind } => kernel_down_type(factor, kind),
            };
            let field_name = resampler_field_name(edge.edge_index);
            inits.push(quote! { #field_name: <#ty>::new() });
        }
        inits
    }

    /// Generate per-node `init()` calls that scale `sample_rate` by the node's
    /// rate annotation. `* N` nodes get `sample_rate * N`, `/ N` nodes get
    /// `sample_rate / N`, and same-rate nodes get `sample_rate` unchanged.
    fn generate_node_init_calls_rate_aware(&self) -> Vec<TokenStream> {
        let mut calls = Vec::new();
        for node in &self.nodes {
            let name = &node.name;
            let scaled = match node.rate {
                NodeRate::Same => quote! { sample_rate },
                NodeRate::Up(f) => {
                    let f = f as f32;
                    quote! { sample_rate * #f }
                }
                NodeRate::Down(d) => {
                    let d = d as f32;
                    quote! { sample_rate / #d }
                }
            };
            if node.array_size.is_some() {
                calls.push(quote! {
                    for __child in self.#name.iter_mut() {
                        ::oscen::SignalProcessor::init(__child, #scaled);
                    }
                });
            } else {
                calls.push(quote! {
                    ::oscen::SignalProcessor::init(&mut self.#name, #scaled);
                });
            }
        }
        calls
    }

    /// Generate `reset()` calls for every cross-rate resampler kernel.
    /// Same-rate edges (`EdgeKernel::None`) produce no field, so they are skipped.
    fn generate_resampler_resets(&self) -> Vec<TokenStream> {
        let mut resets = Vec::new();
        for edge in &self.rate_analysis.edges {
            let f = resampler_field_name(edge.edge_index);
            let stmt = match edge.kernel {
                EdgeKernel::None => continue,
                EdgeKernel::Up { .. } => quote! {
                    ::oscen::resample::StreamUpsampler::reset(&mut self.#f);
                },
                EdgeKernel::Down { .. } => quote! {
                    ::oscen::resample::StreamDownsampler::reset(&mut self.#f);
                },
            };
            resets.push(stmt);
        }
        resets
    }

    /// Generate the `latency_samples()` method on the graph struct.
    ///
    /// Reports the outer-rate latency (in samples) introduced by all multi-rate
    /// downsamplers. Each `Down` edge holds its latency at the **source (high)**
    /// rate; we divide by the resampling factor to convert to the outer rate.
    ///
    /// Up edges' latency is internal to the inner loop and does not affect
    /// outer-rate output timing, so they do not contribute here.
    fn generate_latency_method(&self) -> TokenStream {
        let down_latencies: Vec<_> = self
            .rate_analysis
            .edges
            .iter()
            .filter_map(|e| match e.kernel {
                EdgeKernel::Down { factor, .. } => {
                    let f = resampler_field_name(e.edge_index);
                    let factor_lit = factor as usize;
                    Some(quote! {
                        total += ::oscen::resample::StreamDownsampler::latency_samples(&self.#f) / #factor_lit;
                    })
                }
                _ => None,
            })
            .collect();

        quote! {
            /// Outer-rate latency in samples introduced by all multi-rate downsamplers.
            pub fn latency_samples(&self) -> usize {
                let mut total: usize = 0;
                #(#down_latencies)*
                total
            }
        }
    }

    // ========== Static Graph Generation ==========
    /// Extract the root node identifier from a connection expression
    /// For example: osc.output -> "osc", filter.cutoff -> "filter"
    fn extract_root_node(expr: &ConnectionExpr) -> Option<&syn::Ident> {
        match expr {
            ConnectionExpr::Ident(ident) => Some(ident),
            ConnectionExpr::Field(base, _) => Self::extract_root_node(base),
            ConnectionExpr::MethodCall(base, _, _) => Self::extract_root_node(base),
            ConnectionExpr::ArrayIndex(base, _) => Self::extract_root_node(base),
            ConnectionExpr::Binary(left, _, _) => Self::extract_root_node(left),
            ConnectionExpr::Literal(_) | ConnectionExpr::Call(_, _) => None,
        }
    }

    /// True iff the expression is a pure endpoint reference (no arithmetic,
    /// no function or method calls). Such sources use the fast ConnectEndpoints
    /// path; complex sources are assigned via an evaluated f32.
    fn is_simple_endpoint_source(expr: &ConnectionExpr) -> bool {
        match expr {
            ConnectionExpr::Ident(_) => true,
            ConnectionExpr::Field(base, _) => Self::is_simple_endpoint_source(base),
            ConnectionExpr::ArrayIndex(base, _) => Self::is_simple_endpoint_source(base),
            _ => false,
        }
    }

    /// Walk the expression and push every identifier it mentions (node names,
    /// graph input/output names, function args) into `out`. Used by dependency
    /// tracking to order node processing when sources are compound expressions.
    fn collect_referenced_idents<'a>(
        expr: &'a ConnectionExpr,
        out: &mut Vec<&'a syn::Ident>,
    ) {
        match expr {
            ConnectionExpr::Ident(ident) => out.push(ident),
            ConnectionExpr::Field(base, _) => Self::collect_referenced_idents(base, out),
            ConnectionExpr::ArrayIndex(base, _) => Self::collect_referenced_idents(base, out),
            ConnectionExpr::MethodCall(base, _, _) => Self::collect_referenced_idents(base, out),
            ConnectionExpr::Binary(l, _, r) => {
                Self::collect_referenced_idents(l, out);
                Self::collect_referenced_idents(r, out);
            }
            ConnectionExpr::Call(_, args) => {
                for arg in args {
                    Self::collect_referenced_idents(arg, out);
                }
            }
            ConnectionExpr::Literal(_) => {}
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

        // Build dependencies from connections: dest depends on every node
        // referenced by the source expression (handles arithmetic and calls).
        for conn in &self.connections {
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if !deps.contains_key(dest_node) {
                    continue;
                }
                let mut refs = Vec::new();
                Self::collect_referenced_idents(&conn.source, &mut refs);
                for source_node in refs {
                    if deps.contains_key(source_node) && source_node != dest_node {
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
                    .or_default()
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

    /// Extract the endpoint field name from a simple field-access expression.
    /// For example: osc.output -> Some("output"), filter.cutoff -> Some("cutoff").
    /// Returns None for anything that isn't a bare field access (method calls, etc.).
    fn extract_endpoint_field(expr: &ConnectionExpr) -> Option<&syn::Ident> {
        match expr {
            ConnectionExpr::Field(_, field) => Some(field),
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
            ConnectionExpr::Field(base, field) => {
                let base_tokens = self.connection_expr_to_tokens(base);
                quote! { #base_tokens.#field }
            }
            ConnectionExpr::MethodCall(base, method, args) => {
                let base_tokens = self.connection_expr_to_tokens(base);
                quote! { #base_tokens.#method(#(#args),*) }
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
        // Default: emit all connection assignments (no filtering by edge kernel).
        // Used by the same-rate path; the multi-rate path uses
        // `generate_connection_assignments_for_node_filtered` to skip cross-rate
        // edges (those are bridged by upsamplers/downsamplers).
        self.generate_connection_assignments_for_node_filtered(node_name, |_| true)
    }

    /// Like `generate_connection_assignments_for_node` but only emits assignments
    /// for connections whose `EdgeKernel` matches `keep`. Connection-index
    /// alignment with `RateAnalysis::edges` is preserved by enumerating
    /// `self.connections` in order.
    ///
    /// TODO(multirate-events): cross-rate event edges (i.e. an event endpoint
    /// connected from a `Same`-rate source to a `* N` destination, or vice
    /// versa) are forced to `EdgeKernel::None` by `rate_analysis::analyze` —
    /// the `StreamUpsampler` / `StreamDownsampler` kernels emitted on the
    /// cross-rate path don't type-check against `StaticEventQueue`, and the
    /// macro doesn't yet know endpoint kinds at codegen time to dispatch a
    /// different kernel. As a result, those edges flow through this same-rate
    /// `ConnectEndpoints::connect` dispatch and `EventInstance::frame_offset`
    /// is **not** rescaled across the rate boundary. Events scheduled at
    /// `frame_offset == 0` (the common case after `process_block`'s sub-block
    /// split) are still delivered correctly because the inner loop runs
    /// `process_event_inputs()` once per outer tick on the outer-rate
    /// boundary; events at non-zero offsets fire at the wrong inner tick.
    /// Implementing rescaling requires either (a) threading endpoint-kind
    /// metadata into this codegen path, or (b) adding a `ConnectEndpointsRescaled`
    /// trait variant. Tracked as Phase 5 Task 5.1 in the multi-rate plan and
    /// in the "Known Limitations" section of the multi-rate design spec.
    /// Currently this also implies node-to-node cross-rate event edges (with
    /// no graph-level event endpoint involved) are *not* rerouted to the
    /// same-rate path — they still fail to compile as the cross-rate path
    /// emits `StreamUpsampler` calls for them.
    fn generate_connection_assignments_for_node_filtered<F>(
        &self,
        node_name: &syn::Ident,
        keep: F,
    ) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut assignments = Vec::new();

        // Find all connections where this node is the destination
        for (conn_idx, conn) in self.connections.iter().enumerate() {
            // Filter by edge kernel; same-rate (`None`) and cross-rate (`Up`/`Down`)
            // edges are routed through different code paths in the multi-rate
            // codegen.
            let kernel = self.rate_analysis.edges
                .get(conn_idx)
                .map(|e| e.kernel)
                .unwrap_or(EdgeKernel::None);
            if !keep(&kernel) {
                continue;
            }
            if let Some(dest_node) = Self::extract_root_node(&conn.dest) {
                if dest_node == node_name {
                    // Compound sources (arithmetic, function/method calls) don't have
                    // a single root endpoint. Evaluate them as f32 and route via
                    // ConnectEndpoints<f32, _>.
                    if !Self::is_simple_endpoint_source(&conn.source) {
                        if let Some(dest_field) = Self::extract_endpoint_field(&conn.dest) {
                            let src_tokens = self.connection_expr_to_tokens(&conn.source);
                            if let Some(dest_size) = self.get_node_array_size(dest_node) {
                                assignments.push(quote! {
                                    {
                                        let __src: f32 = #src_tokens;
                                        for i in 0..#dest_size {
                                            <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                                &__src,
                                                &mut self.#dest_node[i].#dest_field,
                                            );
                                        }
                                    }
                                });
                            } else {
                                assignments.push(quote! {
                                    <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                        &(#src_tokens),
                                        &mut self.#dest_node.#dest_field,
                                    );
                                });
                            }
                        }
                        continue;
                    }

                    // This connection feeds into the current node
                    if let Some(source_ident) = Self::extract_root_node(&conn.source) {
                        let source_field = Self::extract_endpoint_field(&conn.source);

                        if let Some(dest_field) = Self::extract_endpoint_field(&conn.dest) {
                            // Check if source is a graph input (not a node)
                            let source_is_graph_input = self.inputs.iter().any(|i| i.name == *source_ident);

                            // Skip voice array marker connections (like .voices -> array.endpoint)
                            // These have special routing handled by the array output node
                            if let Some(field) = source_field {
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
                            // For ramped graph inputs, we need to access .current to get the f32 value
                            let source_access = if source_is_graph_input && source_field.is_none() && self.is_ramped_input(source_ident).is_some() {
                                // Ramped graph input: read .current
                                quote! { .current }
                            } else if let Some(field) = source_field {
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

    /// Generate the shared process body: connection assignments, node processing,
    /// and output routing. Used by both `process()` and `__advance_one_frame()`.
    fn generate_process_body(&self) -> Result<Vec<TokenStream>> {
        let sorted_nodes = self.topological_sort_nodes()?;

        let mut process_body = Vec::new();

        for node_name in &sorted_nodes {
            // Connection assignments that feed into this node
            let assignments = self.generate_connection_assignments_for_node(node_name);
            process_body.extend(assignments);

            // process_event_inputs() + process() for each node
            process_body.push(self.emit_node_process_call(node_name));
        }

        // Assignments for connections to graph outputs (no edge-kernel filtering).
        process_body.extend(self.generate_graph_output_assignments_filtered(|_| true));

        Ok(process_body)
    }

    /// Emit `process_event_inputs()` + `process()` for a single node (handles
    /// both scalar and array nodes).
    fn emit_node_process_call(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process_event_inputs();
                    self.#node_name[i].process();
                }
            }
        } else {
            quote! {
                self.#node_name.process_event_inputs();
                self.#node_name.process();
            }
        }
    }

    /// Emit only `process()` for a single node (no event-input handling). Used
    /// inside the multi-rate inner loop where `process_event_inputs()` should
    /// run once per outer tick rather than per inner sample.
    fn emit_node_process_only(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process();
                }
            }
        } else {
            quote! {
                self.#node_name.process();
            }
        }
    }

    /// Emit `process_event_inputs()` for a single node (no `process()`).
    fn emit_node_process_event_inputs(&self, node_name: &syn::Ident) -> TokenStream {
        if let Some(array_size) = self.get_node_array_size(node_name) {
            quote! {
                for i in 0..#array_size {
                    self.#node_name[i].process_event_inputs();
                }
            }
        } else {
            quote! {
                self.#node_name.process_event_inputs();
            }
        }
    }

    /// Emit assignments for connections that target graph outputs.
    /// `keep` filters by edge kernel — multi-rate codegen passes
    /// `|k| matches!(k, EdgeKernel::None)` to skip cross-rate edges, which are
    /// finalized via the per-edge downsampler instead.
    fn generate_graph_output_assignments_filtered<F>(&self, keep: F) -> Vec<TokenStream>
    where
        F: Fn(&EdgeKernel) -> bool,
    {
        let mut out = Vec::new();
        for (conn_idx, conn) in self.connections.iter().enumerate() {
            let kernel = self.rate_analysis.edges
                .get(conn_idx)
                .map(|e| e.kernel)
                .unwrap_or(EdgeKernel::None);
            if !keep(&kernel) {
                continue;
            }
            if let Some(dest_ident) = Self::extract_root_node(&conn.dest) {
                if let Some(output_decl) = self.outputs.iter().find(|o| o.name == *dest_ident) {
                    let source_node = Self::extract_root_node(&conn.source);
                    let source_field = Self::extract_endpoint_field(&conn.source);
                    let is_simple_source = source_node.is_some() && source_field.is_some();

                    match output_decl.kind {
                        EndpointKind::Stream | EndpointKind::Value => {
                            if is_simple_source {
                                let source_node = source_node.unwrap();
                                let source_field = source_field.unwrap();
                                if let Some(_src_array_size) = self.get_node_array_size(source_node) {
                                    out.push(quote! {
                                        self.#dest_ident = self.#source_node.iter().map(|n| n.#source_field).sum();
                                    });
                                } else {
                                    out.push(quote! {
                                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                            &self.#source_node.#source_field,
                                            &mut self.#dest_ident
                                        );
                                    });
                                }
                            } else {
                                let source_tokens = self.connection_expr_to_tokens(&conn.source);
                                out.push(quote! {
                                    self.#dest_ident = #source_tokens;
                                });
                            }
                        }
                        EndpointKind::Event => {
                            if is_simple_source {
                                let source_node = source_node.unwrap();
                                let source_field = source_field.unwrap();
                                if let Some(array_size) = self.get_node_array_size(source_node) {
                                    out.push(quote! {
                                        self.#dest_ident.clear();
                                        for i in 0..#array_size {
                                            for event in self.#source_node[i].#source_field.iter() {
                                                let _ = self.#dest_ident.try_push(event.clone());
                                            }
                                        }
                                    });
                                } else {
                                    out.push(quote! {
                                        <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                                            &self.#source_node.#source_field,
                                            &mut self.#dest_ident
                                        );
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Generate event queue clearing statements for graph-level event inputs/outputs.
    fn generate_event_clearing(&self) -> Vec<TokenStream> {
        let mut clearing = Vec::new();
        for input in &self.inputs {
            if input.kind == EndpointKind::Event {
                let field_name = &input.name;
                clearing.push(quote! {
                    self.#field_name.clear();
                });
            }
        }
        for output in &self.outputs {
            if output.kind == EndpointKind::Event {
                let field_name = &output.name;
                clearing.push(quote! {
                    self.#field_name.clear();
                });
            }
        }
        clearing
    }

    /// Generate the static process() method for compile-time graphs.
    /// Wraps the shared process body with tick_ramps() and event clearing.
    fn generate_static_process(&self) -> Result<TokenStream> {
        let process_body = self.generate_process_body()?;
        let event_clearing = self.generate_event_clearing();

        Ok(quote! {
            #[inline(always)]
            pub fn process(&mut self) {
                use ::oscen::SignalProcessor as _;

                // Advance ramped value inputs
                self.tick_ramps();

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

    // ========== Block Processing Methods ==========

    /// Generate the `__advance_one_frame()` private method.
    /// Dispatches to either the same-rate fast path or the multi-rate variant
    /// based on `RateAnalysis::max_factor`. Same-rate graphs (max_factor == 1)
    /// produce identical code to before the multi-rate work landed.
    fn generate_advance_one_frame(&self) -> Result<TokenStream> {
        if self.rate_analysis.max_factor <= 1 {
            self.generate_advance_one_frame_same_rate()
        } else {
            self.generate_advance_one_frame_multirate()
        }
    }

    /// Same-rate fast path: a single per-frame call to the shared process body.
    /// This is the original `__advance_one_frame` implementation; behavior must
    /// be identical to pre-multi-rate codegen.
    fn generate_advance_one_frame_same_rate(&self) -> Result<TokenStream> {
        let process_body = self.generate_process_body()?;

        // Read stream inputs from block buffers
        let stream_input_reads: Vec<_> = self.inputs.iter()
            .filter(|i| i.kind == EndpointKind::Stream)
            .map(|i| {
                let name = &i.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#name = self.#block_name[__frame]; }
            })
            .collect();

        // Write stream outputs to block buffers
        let stream_output_writes: Vec<_> = self.outputs.iter()
            .filter(|o| o.kind == EndpointKind::Stream)
            .map(|o| {
                let name = &o.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#block_name[__frame] = self.#name; }
            })
            .collect();

        Ok(quote! {
            #[inline(always)]
            #[allow(unused_variables)]
            fn __advance_one_frame(&mut self, __frame: usize) {
                use ::oscen::SignalProcessor as _;

                #(#stream_input_reads)*

                self.tick_ramps();

                #(#process_body)*

                #(#stream_output_writes)*
            }
        })
    }

    /// Multi-rate variant of `__advance_one_frame`. Splits the graph into
    /// outer-rate (`Same`) nodes and oversampled (`Up(N)`) nodes, runs the
    /// outer-rate ones once per outer tick, upsamples each cross-rate Up edge,
    /// runs the inner loop ×N times for the oversampled nodes (with same-rate
    /// inner-rate connections still emitted normally), captures Down-edge
    /// sources into per-edge buffers, then downsamples them once per outer
    /// tick into their destination fields. Same-rate connections to graph
    /// outputs run after the inner loop so they see post-downsampling values.
    fn generate_advance_one_frame_multirate(&self) -> Result<TokenStream> {
        let max_factor = self.rate_analysis.max_factor as usize;
        let sorted_nodes = self.topological_sort_nodes()?;

        // Bucket nodes by rate.
        let outer_node_names: Vec<syn::Ident> = sorted_nodes
            .iter()
            .filter(|name| matches!(self.node_rate(name), NodeRate::Same))
            .cloned()
            .collect();
        let inner_node_names: Vec<syn::Ident> = sorted_nodes
            .iter()
            .filter(|name| matches!(self.node_rate(name), NodeRate::Up(_)))
            .cloned()
            .collect();

        // Read stream inputs from block buffers (per outer tick).
        let stream_input_reads: Vec<_> = self.inputs.iter()
            .filter(|i| i.kind == EndpointKind::Stream)
            .map(|i| {
                let name = &i.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#name = self.#block_name[__frame]; }
            })
            .collect();

        // Write stream outputs to block buffers (per outer tick).
        let stream_output_writes: Vec<_> = self.outputs.iter()
            .filter(|o| o.kind == EndpointKind::Stream)
            .map(|o| {
                let name = &o.name;
                let block_name = syn::Ident::new(&format!("{}_block", name), name.span());
                quote! { self.#block_name[__frame] = self.#name; }
            })
            .collect();

        // Step 3: Outer-rate node processing. Each outer-rate node sees only
        // its same-rate (`EdgeKernel::None`) incoming assignments — cross-rate
        // edges are routed via resamplers below.
        let mut outer_process: Vec<TokenStream> = Vec::new();
        for node_name in &outer_node_names {
            let assignments = self.generate_connection_assignments_for_node_filtered(
                node_name,
                |k| matches!(k, EdgeKernel::None),
            );
            outer_process.extend(assignments);
            outer_process.push(self.emit_node_process_call(node_name));
        }

        // Step 4: Per-edge upsample warmup for `EdgeKernel::Up` connections.
        // Generates one fixed-size [f32; N] buffer per edge, populated by the
        // edge's upsampler from the source's freshly-computed outer-rate value.
        let mut up_decls: Vec<TokenStream> = Vec::new();
        for edge in &self.rate_analysis.edges {
            if let EdgeKernel::Up { factor, .. } = edge.kernel {
                let factor_us = factor as usize;
                let buf = up_buf_name(edge.edge_index);
                let field = resampler_field_name(edge.edge_index);
                let conn = &self.connections[edge.edge_index];
                let src_value = self.connection_source_value_expr(&conn.source);
                up_decls.push(quote! {
                    let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
                    {
                        let __src_val: f32 = #src_value;
                        ::oscen::resample::StreamUpsampler::upsample(
                            &mut self.#field,
                            __src_val,
                            &mut #buf,
                        );
                    }
                });
            }
        }

        // Step 5: Per-edge accumulator buffers for `EdgeKernel::Down`
        // connections. Filled inside the inner loop and consumed afterwards.
        let mut down_decls: Vec<TokenStream> = Vec::new();
        for edge in &self.rate_analysis.edges {
            if let EdgeKernel::Down { factor, .. } = edge.kernel {
                let factor_us = factor as usize;
                let buf = down_buf_name(edge.edge_index);
                down_decls.push(quote! {
                    let mut #buf: [f32; #factor_us] = [0.0; #factor_us];
                });
            }
        }

        // Step 6 inner-loop body. Runs ×N times.
        //
        //   a) For each Up edge whose dest is an inner-rate node: write the
        //      precomputed upsampled sample into the dest's input field.
        //   b) For each inner-rate node, in topo order: emit its same-rate
        //      incoming assignments and call `process()`. We split
        //      `process_event_inputs()` out and run it once per outer tick (see
        //      below) — events fire at outer rate.
        //   c) For each Down edge: capture the source's current inner-rate
        //      output into the per-edge accumulator buffer.

        // Run process_event_inputs() for inner-rate nodes once per outer tick,
        // before the inner loop, so events arrive on the outer-rate boundary.
        let inner_event_input_calls: Vec<TokenStream> = inner_node_names.iter()
            .map(|n| self.emit_node_process_event_inputs(n))
            .collect();

        let mut inner_writes: Vec<TokenStream> = Vec::new();
        for edge in &self.rate_analysis.edges {
            if let EdgeKernel::Up { .. } = edge.kernel {
                let buf = up_buf_name(edge.edge_index);
                let conn = &self.connections[edge.edge_index];
                let dest_assign = self.connection_dest_field_assign(&conn.dest, &quote! { #buf[__inner] });
                inner_writes.push(dest_assign);
            }
        }

        let mut inner_node_runs: Vec<TokenStream> = Vec::new();
        for node_name in &inner_node_names {
            let assignments = self.generate_connection_assignments_for_node_filtered(
                node_name,
                |k| matches!(k, EdgeKernel::None),
            );
            inner_node_runs.extend(assignments);
            inner_node_runs.push(self.emit_node_process_only(node_name));
        }

        let mut down_captures: Vec<TokenStream> = Vec::new();
        for edge in &self.rate_analysis.edges {
            if let EdgeKernel::Down { .. } = edge.kernel {
                let buf = down_buf_name(edge.edge_index);
                let conn = &self.connections[edge.edge_index];
                let src_value = self.connection_source_value_expr(&conn.source);
                down_captures.push(quote! {
                    #buf[__inner] = #src_value;
                });
            }
        }

        // Step 7: Finalize Down edges by calling the per-edge downsampler and
        // writing the result into the dest field (which may be an outer-rate
        // node input or a graph output).
        let mut down_finalizes: Vec<TokenStream> = Vec::new();
        for edge in &self.rate_analysis.edges {
            if let EdgeKernel::Down { .. } = edge.kernel {
                let buf = down_buf_name(edge.edge_index);
                let field = resampler_field_name(edge.edge_index);
                let conn = &self.connections[edge.edge_index];
                let dest_assign = self.connection_dest_field_assign(
                    &conn.dest,
                    &quote! {
                        ::oscen::resample::StreamDownsampler::downsample(
                            &mut self.#field,
                            &#buf,
                        )
                    },
                );
                down_finalizes.push(dest_assign);
            }
        }

        // Step 8: Same-rate connection assignments to graph outputs (skip
        // cross-rate Down edges — those were finalized via downsamplers).
        let same_rate_output_trailer = self.generate_graph_output_assignments_filtered(
            |k| matches!(k, EdgeKernel::None),
        );

        Ok(quote! {
            #[inline(always)]
            #[allow(unused_variables, unused_mut)]
            fn __advance_one_frame(&mut self, __frame: usize) {
                use ::oscen::SignalProcessor as _;

                // 1. Read stream inputs from block buffers (outer-rate).
                #(#stream_input_reads)*

                // 2. Tick ramped value inputs at outer rate.
                self.tick_ramps();

                // 3. Outer-rate (Same) nodes process once per outer tick.
                #(#outer_process)*

                // 4. Per-edge upsample warmup for cross-rate Up edges.
                #(#up_decls)*

                // 5. Per-edge accumulator buffers for cross-rate Down edges.
                #(#down_decls)*

                // 6a. Run process_event_inputs() once per outer tick for inner
                // nodes — events fire at outer rate to keep dispatch cheap.
                #(#inner_event_input_calls)*

                // 6. Inner loop: ×N nodes run N times per outer tick.
                for __inner in 0..#max_factor {
                    #(#inner_writes)*
                    #(#inner_node_runs)*
                    #(#down_captures)*
                }

                // 7. Downsample once per outer tick into dest fields.
                #(#down_finalizes)*

                // 8. Same-rate trailer assignments (e.g., to graph outputs).
                #(#same_rate_output_trailer)*

                // 9. Write stream outputs to block buffers (outer-rate).
                #(#stream_output_writes)*
            }
        })
    }

    /// Look up the rate annotation for a node by name. Falls back to `Same`
    /// for unknown names (defensive — should never happen for nodes that
    /// passed type checking).
    fn node_rate(&self, name: &syn::Ident) -> NodeRate {
        self.rate_analysis.node_rates
            .get(&name.to_string())
            .copied()
            .unwrap_or(NodeRate::Same)
    }

    /// Build an `f32`-valued expression for a connection's source. Handles the
    /// common simple cases (graph input, `node.field`) by reading from the
    /// graph state, and falls back to evaluating a compound expression.
    fn connection_source_value_expr(&self, source: &ConnectionExpr) -> TokenStream {
        // Compound or non-trivial sources: let the existing token converter
        // produce an f32 expression.
        if !Self::is_simple_endpoint_source(source) {
            let toks = self.connection_expr_to_tokens(source);
            return quote! { (#toks) as f32 };
        }

        // Simple endpoint source. Read via ConnectEndpoints into a local f32
        // so we don't have to know the source's exact wrapper type.
        let toks = self.connection_expr_to_tokens(source);
        quote! {
            {
                let mut __src: f32 = 0.0;
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &#toks,
                    &mut __src,
                );
                __src
            }
        }
    }

    /// Build an assignment `dest <- value` for a connection's dest. The dest
    /// may be a node-input field (typed wrapper around f32) or a graph output
    /// (plain f32 / event queue). Uses `ConnectEndpoints` for node inputs to
    /// be wrapper-type-agnostic; uses direct assignment for graph outputs.
    fn connection_dest_field_assign(&self, dest: &ConnectionExpr, value: &TokenStream) -> TokenStream {
        // Graph output (Ident only, no field): direct assignment.
        if let Some(dest_ident) = Self::extract_root_node(dest) {
            if self.outputs.iter().any(|o| o.name == *dest_ident) {
                if matches!(dest, ConnectionExpr::Ident(_)) {
                    return quote! { self.#dest_ident = #value; };
                }
            }
        }

        // Node input: bridge via ConnectEndpoints to handle typed wrappers.
        let dest_toks = self.connection_expr_to_tokens(dest);
        quote! {
            {
                let __dst_val: f32 = #value;
                <() as ::oscen::graph::ConnectEndpoints<_, _>>::connect(
                    &__dst_val,
                    &mut #dest_toks,
                );
            }
        }
    }

    /// Generate the `process_block()` public method.
    /// If the graph has event inputs, generates sub-block splitting at event boundaries.
    /// Otherwise, generates a simple tight loop.
    fn generate_static_process_block(&self) -> Result<TokenStream> {
        let has_event_inputs = self.inputs.iter().any(|i| i.kind == EndpointKind::Event);

        if !has_event_inputs {
            // No events: simple tight loop
            return Ok(quote! {
                /// Process a block of `frames` samples.
                /// Stream inputs should be written to `*_block` arrays before calling.
                /// Stream outputs will be available in `*_block` arrays after calling.
                pub fn process_block(&mut self, frames: usize) {
                    debug_assert!(frames <= Self::MAX_BLOCK_SIZE);
                    for __frame in 0..frames {
                        self.__advance_one_frame(__frame);
                    }
                }
            });
        }

        // Event inputs exist: generate sub-block splitting

        // Stage: copy events to local sorted storage, drain originals
        let event_inputs: Vec<_> = self.inputs.iter()
            .filter(|i| i.kind == EndpointKind::Event)
            .collect();

        let staging: Vec<_> = event_inputs.iter().map(|input| {
            let name = &input.name;
            let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
            let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
            quote! {
                let mut #staged_name: ::oscen::graph::StaticEventQueue =
                    ::oscen::graph::StaticEventQueue::new();
                for __e in self.#name.iter() {
                    let _ = #staged_name.try_push(__e.clone());
                }
                self.#name.clear();
                #staged_name.sort_unstable_by_key(|__e| __e.frame_offset);
                let mut #cursor_name: usize = 0;
            }
        }).collect();

        // Find next event boundary across all event inputs
        let boundary_checks: Vec<_> = event_inputs.iter().map(|input| {
            let name = &input.name;
            let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
            let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
            quote! {
                if #cursor_name < #staged_name.len() {
                    __next_event = __next_event.min(
                        (#staged_name[#cursor_name].frame_offset as usize).max(__frame)
                    );
                }
            }
        }).collect();

        // Push events at boundary
        let event_pushes: Vec<_> = event_inputs.iter().map(|input| {
            let name = &input.name;
            let staged_name = syn::Ident::new(&format!("__staged_{}", name), name.span());
            let cursor_name = syn::Ident::new(&format!("__cursor_{}", name), name.span());
            quote! {
                while #cursor_name < #staged_name.len()
                    && #staged_name[#cursor_name].frame_offset == __frame as u32
                {
                    let _ = self.#name.try_push(#staged_name[#cursor_name].clone());
                    #cursor_name += 1;
                }
            }
        }).collect();

        // Clear event queues after event frame
        let event_clearing = self.generate_event_clearing();

        Ok(quote! {
            /// Process a block of `frames` samples with sub-block splitting at event boundaries.
            /// Stream inputs should be written to `*_block` arrays before calling.
            /// Stream outputs will be available in `*_block` arrays after calling.
            /// Events should be pushed to event input queues with appropriate `frame_offset` values.
            pub fn process_block(&mut self, frames: usize) {
                debug_assert!(frames <= Self::MAX_BLOCK_SIZE);

                // Stage: copy events to local sorted storage, drain originals
                #(#staging)*

                let mut __frame: usize = 0;
                while __frame < frames {
                    // Find next event boundary across all event inputs
                    let mut __next_event: usize = frames;
                    #(#boundary_checks)*

                    // Tight loop up to next event boundary (no events, no branches)
                    while __frame < __next_event {
                        self.__advance_one_frame(__frame);
                        __frame += 1;
                    }

                    if __frame >= frames { break; }

                    // Push events at this boundary into graph-level queues
                    #(#event_pushes)*

                    // Process the event frame
                    self.__advance_one_frame(__frame);
                    __frame += 1;

                    // Clear event queues so next sub-block starts clean
                    #(#event_clearing)*
                }
            }
        })
    }

    // ========== Value Ramp Methods ==========

    /// Generate tick_ramps() method that advances all ramped value inputs.
    /// Optimized to check active_ramps counter first for O(1) steady-state overhead.
    fn generate_tick_ramps_method(&self) -> TokenStream {
        let ramped: Vec<_> = self
            .inputs
            .iter()
            .filter(|i| i.kind == EndpointKind::Value && self.is_ramped_input(&i.name).is_some())
            .map(|i| &i.name)
            .collect();

        if ramped.is_empty() {
            return quote! {
                #[inline(always)]
                fn tick_ramps(&mut self) {}
            };
        }

        let tick_stmts: Vec<_> = ramped
            .iter()
            .map(|name| quote! {
                if self.#name.tick() {
                    self.active_ramps -= 1;
                }
            })
            .collect();

        quote! {
            #[inline(always)]
            fn tick_ramps(&mut self) {
                if self.active_ramps > 0 {
                    #(#tick_stmts)*
                }
            }
        }
    }

    /// Generate setter methods for value inputs.
    /// For ramped inputs: set_X(value) uses default ramp, set_X_with_ramp(value, frames) uses custom ramp.
    /// For non-ramped inputs: set_X(value) sets immediately.
    /// Ramped setters update the active_ramps counter for O(1) steady-state tick_ramps() overhead.
    fn generate_value_setter_methods(&self) -> Vec<TokenStream> {
        self.inputs
            .iter()
            .filter(|i| i.kind == EndpointKind::Value)
            .map(|input| {
                let name = &input.name;
                let set_name = syn::Ident::new(&format!("set_{}", name), name.span());

                if let Some(default_frames) = self.is_ramped_input(name) {
                    let set_ramp_name =
                        syn::Ident::new(&format!("set_{}_with_ramp", name), name.span());
                    let set_immediate_name =
                        syn::Ident::new(&format!("set_{}_immediate", name), name.span());
                    quote! {
                        /// Set the value with the default ramp duration.
                        /// No-op if target is already the same (safe to call every frame).
                        #[inline]
                        pub fn #set_name(&mut self, value: f32) {
                            // Only start a new ramp if target actually changed
                            if value != self.#name.target {
                                if !self.#name.is_ramping() {
                                    self.active_ramps += 1;
                                }
                                self.#name.set_with_ramp(value, #default_frames as u32);
                            }
                        }

                        /// Set the value with a custom ramp duration in frames.
                        /// No-op if target is already the same (safe to call every frame).
                        #[inline]
                        pub fn #set_ramp_name(&mut self, value: f32, frames: u32) {
                            // Only start a new ramp if target actually changed
                            if value != self.#name.target {
                                if frames > 0 && !self.#name.is_ramping() {
                                    self.active_ramps += 1;
                                }
                                self.#name.set_with_ramp(value, frames);
                            }
                        }

                        /// Set the value immediately without ramping.
                        #[inline]
                        pub fn #set_immediate_name(&mut self, value: f32) {
                            if self.#name.is_ramping() {
                                self.active_ramps -= 1;
                            }
                            self.#name.set_immediate(value);
                        }
                    }
                } else {
                    quote! {
                        /// Set the value immediately.
                        #[inline]
                        pub fn #set_name(&mut self, value: f32) {
                            self.#name = value;
                        }
                    }
                }
            })
            .collect()
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
                        // Calculate skew factor so that `center` is at normalized 0.5
                        // Formula: factor = 0.5.log((center - min) / (max - min))
                        quote! {
                            ::nih_plug::prelude::FloatRange::Skewed {
                                min: #min,
                                max: #max,
                                factor: 0.5f32.log((#center - #min) / (#max - #min)),
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

            // Add smoother only if explicitly requested via `smoother:` attribute.
            // Ramped inputs use oscen's ValueRampState instead.
            // Non-ramped inputs without explicit smoother use raw values — with
            // block processing, per-sample NIH-plug smoothers called once per block
            // produce staircase artifacts.
            let is_ramped = self.is_ramped_input(field_name).is_some();
            if !is_ramped {
                let smoother_ms = input.spec.as_ref()
                    .and_then(|s| s.smoother.clone());
                if let Some(smoother_val) = smoother_ms {
                    param_builder = quote! {
                        #param_builder
                            .with_smoother(::nih_plug::prelude::SmoothingStyle::Linear(#smoother_val))
                    };
                }
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
            let set_name = syn::Ident::new(&format!("set_{}", field_name), field_name.span());
            if self.is_ramped_input(field_name).is_some() {
                // Ramped inputs: use setter with raw value (oscen handles smoothing)
                // The setter is smart and won't restart ramp if target unchanged
                quote! {
                    graph.#set_name(self.#field_name.value());
                }
            } else {
                // Non-ramped inputs: use raw value (safe for block-rate sync)
                quote! {
                    graph.#field_name = self.#field_name.value();
                }
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
                /// Sync parameter values to the graph (call once per block)
                #[inline(always)]
                pub fn sync_to(&self, graph: &mut #graph_name) {
                    #(#sync_assignments)*
                }
            }
        }
    }

    /// Check if this graph has any ramped inputs
    fn has_ramped_inputs(&self) -> bool {
        self.inputs.iter().any(|i| {
            i.kind == EndpointKind::Value && self.is_ramped_input(&i.name).is_some()
        })
    }

    fn generate_static_struct(&self, name: &syn::Ident) -> Result<TokenStream> {
        let mut fields = vec![quote! { sample_rate: f32 }];

        // Add active_ramps counter if there are ramped inputs
        if self.has_ramped_inputs() {
            fields.push(quote! { active_ramps: u32 });
        }

        // Add input fields
        for input in &self.inputs {
            let field_name = &input.name;
            let ty = match input.kind {
                EndpointKind::Value => {
                    if self.is_ramped_input(field_name).is_some() {
                        quote! { ::oscen::graph::ValueRampState }
                    } else {
                        quote! { f32 }
                    }
                }
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
                EndpointKind::Stream => quote! { f32 },
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream inputs
            if input.kind == EndpointKind::Stream {
                let block_name = syn::Ident::new(&format!("{}_block", field_name), field_name.span());
                fields.push(quote! { pub #block_name: [f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] });
            }
        }

        // Add output fields (store actual values for static graphs)
        for output in &self.outputs {
            let field_name = &output.name;
            let ty = match output.kind {
                EndpointKind::Stream => quote! { f32 },
                EndpointKind::Value => quote! { f32 },
                EndpointKind::Event => quote! { ::oscen::graph::StaticEventQueue },
            };
            fields.push(quote! { pub #field_name: #ty });

            // Block buffer for stream outputs
            if output.kind == EndpointKind::Stream {
                let block_name = syn::Ident::new(&format!("{}_block", field_name), field_name.span());
                fields.push(quote! { pub #block_name: [f32; ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE] });
            }
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

        // Per-edge resampler kernels for cross-rate connections (Task 4.2).
        // Connection-routing logic (Task 4.4) consumes these fields; for now
        // they are present and zero-initialized but unused for same-rate graphs.
        let resampler_fields = self.generate_resampler_fields();
        let resampler_inits = self.generate_resampler_inits();

        // For compile-time graphs, generate a static process() method
        let process_method = self.generate_static_process()?;
        let advance_one_frame_method = self.generate_advance_one_frame()?;
        let process_block_method = self.generate_static_process_block()?;
        let get_stream_output_method = self.generate_static_get_stream_output();
        let clear_event_outputs_method = self.generate_static_clear_event_outputs();
        let process_event_inputs_method = self.generate_static_process_event_inputs();
        let event_handler_methods = self.generate_static_event_handler_methods();
        let tick_ramps_method = self.generate_tick_ramps_method();
        let value_setter_methods = self.generate_value_setter_methods();
        let latency_method = self.generate_latency_method();

        // Generate init() calls for each node, scaling `sample_rate` by the
        // node's rate annotation (`* N` -> ×N, `/ N` -> ÷N, default -> unchanged).
        let node_init_calls = self.generate_node_init_calls_rate_aware();
        // Reset every cross-rate resampler kernel so re-initialization clears
        // any per-edge filter state (delay lines, IIR taps, latched samples).
        let resampler_resets = self.generate_resampler_resets();

        // Generate NIH-plug params struct if nih_params flag is set
        let nih_params_output = if self.nih_params {
            self.generate_nih_params_struct(name)
        } else {
            quote! {}
        };

        // If there are any cross-rate edges we append a leading comma to the
        // tail so the existing `#struct_init` (which has no trailing comma)
        // chains cleanly into the resampler inits.
        let resampler_init_tail = if resampler_inits.is_empty() {
            quote! {}
        } else {
            quote! { , #(#resampler_inits),* }
        };

        Ok(quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #name {
                #(#fields,)*
                #(#resampler_fields,)*
            }

            impl #name {
                /// Maximum block size for `process_block()`.
                pub const MAX_BLOCK_SIZE: usize = ::oscen::graph::DEFAULT_MAX_BLOCK_SIZE;

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
                        #resampler_init_tail
                    }
                }

                #process_method

                #advance_one_frame_method

                #process_block_method

                #get_stream_output_method

                #clear_event_outputs_method

                #process_event_inputs_method

                #(#event_handler_methods)*

                #tick_ramps_method

                #(#value_setter_methods)*

                #latency_method
            }

            // Generate SignalProcessor implementation for compile-time graphs
            impl ::oscen::SignalProcessor for #name {
                fn init(&mut self, sample_rate: f32) {
                    self.sample_rate = sample_rate;
                    // Call init() on all child nodes, scaling sample_rate by
                    // each node's rate annotation.
                    #(#node_init_calls)*
                    // Reset every cross-rate resampler kernel.
                    #(#resampler_resets)*
                }

                fn process(&mut self) {
                    // This is already implemented in the impl block above
                }
            }

            #nih_params_output
        })
    }
}
