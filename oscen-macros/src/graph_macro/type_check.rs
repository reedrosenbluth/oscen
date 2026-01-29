use super::ast::*;
use std::collections::HashMap;
use syn::{Ident, Result};

/// Tracks the inferred types of expressions in the graph
pub struct TypeContext {
    /// Known types for inputs
    inputs: HashMap<String, EndpointKind>,
    /// Known types for outputs
    outputs: HashMap<String, EndpointKind>,
    /// Node endpoint types: (node_name, endpoint_name) -> EndpointKind
    node_endpoints: HashMap<(String, String), EndpointKind>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            inputs: HashMap::new(),
            outputs: HashMap::new(),
            node_endpoints: HashMap::new(),
        }
    }

    /// Register an input declaration
    pub fn register_input(&mut self, name: &Ident, kind: EndpointKind) {
        self.inputs.insert(name.to_string(), kind);
    }

    /// Register an output declaration
    pub fn register_output(&mut self, name: &Ident, kind: EndpointKind) {
        self.outputs.insert(name.to_string(), kind);
    }

    /// Register a node endpoint (from connection analysis or explicit declaration)
    pub fn register_node_endpoint(&mut self, node_name: &str, endpoint_name: &str, kind: EndpointKind) {
        self.node_endpoints.insert((node_name.to_string(), endpoint_name.to_string()), kind);
    }

    /// Get the type of a node endpoint if known
    pub fn get_node_endpoint_type(&self, node_name: &str, endpoint_name: &str) -> Option<EndpointKind> {
        self.node_endpoints.get(&(node_name.to_string(), endpoint_name.to_string())).copied()
    }

    /// Infer the type of a connection expression
    pub fn infer_type(&self, expr: &ConnectionExpr) -> Option<EndpointKind> {
        match expr {
            ConnectionExpr::Ident(ident) => {
                let name = ident.to_string();
                // Check if it's a known input or output
                self.inputs
                    .get(&name)
                    .or_else(|| self.outputs.get(&name))
                    .copied()
            }
            ConnectionExpr::ArrayIndex(array_expr, _idx) => {
                // Array indexing preserves the type of the base expression
                self.infer_type(array_expr)
            }
            ConnectionExpr::Method(obj, method, _args) => {
                let method_name = method.to_string();

                // Try to look up the node endpoint type from our registry
                if let ConnectionExpr::Ident(node_name) = &**obj {
                    if let Some(kind) = self.get_node_endpoint_type(&node_name.to_string(), &method_name) {
                        return Some(kind);
                    }
                }

                // Fallback: check if it's a graph input/output being accessed
                // (shouldn't normally happen, but handle gracefully)
                None
            }
            ConnectionExpr::Binary(left, _op, right) => {
                // Arithmetic operations on streams produce streams
                // Operations involving values produce values
                let left_type = self.infer_type(left)?;
                let right_type = self.infer_type(right)?;

                match (left_type, right_type) {
                    // Stream operations preserve stream type
                    (EndpointKind::Stream, EndpointKind::Stream) => Some(EndpointKind::Stream),
                    (EndpointKind::Stream, EndpointKind::Value) => Some(EndpointKind::Stream),
                    (EndpointKind::Value, EndpointKind::Stream) => Some(EndpointKind::Stream),

                    // Value operations produce values
                    (EndpointKind::Value, EndpointKind::Value) => Some(EndpointKind::Value),

                    // Events can't be combined with arithmetic
                    (EndpointKind::Event, _) | (_, EndpointKind::Event) => None,
                }
            }
            ConnectionExpr::Literal(_) => {
                // Literals are treated as values
                Some(EndpointKind::Value)
            }
            ConnectionExpr::Call(_func, _args) => {
                // Can't infer function return types without more context
                None
            }
        }
    }

    /// Validate that a connection is type-safe
    pub fn validate_connection(
        &self,
        source: &ConnectionExpr,
        dest: &ConnectionExpr,
    ) -> Result<()> {
        let source_type = self.infer_type(source);
        let dest_type = self.infer_type(dest);

        match (source_type, dest_type) {
            (Some(src), Some(dst)) => {
                // Check if types are compatible
                let compatible = match (src, dst) {
                    // Exact matches
                    (EndpointKind::Stream, EndpointKind::Stream) => true,
                    (EndpointKind::Value, EndpointKind::Value) => true,
                    (EndpointKind::Event, EndpointKind::Event) => true,
                    // Stream sources can connect to Value destinations (auto-conversion)
                    (EndpointKind::Stream, EndpointKind::Value) => true,
                    // Value sources can connect to Stream destinations (constant signal)
                    (EndpointKind::Value, EndpointKind::Stream) => true,
                    // Everything else is incompatible
                    _ => false,
                };

                if !compatible {
                    // Create a descriptive error message
                    let msg = format!(
                        "Type mismatch in connection: source is {:?} but destination expects {:?}",
                        src, dst
                    );

                    // Try to create a helpful error pointing to the connection
                    return Err(syn::Error::new(proc_macro2::Span::call_site(), msg));
                }
            }
            // If we can't infer types, let Rust's type system handle it
            _ => {}
        }

        Ok(())
    }

    /// Validate that a destination expression can receive input
    /// This is now mostly delegated to Rust's type system since we don't use string matching
    pub fn validate_destination(&self, expr: &ConnectionExpr) -> Result<()> {
        // Allow graph outputs as destinations (common case)
        if let ConnectionExpr::Ident(ident) = expr {
            if self.outputs.contains_key(&ident.to_string()) {
                return Ok(());
            }
        }

        // Everything else is validated by type compatibility check
        Ok(())
    }
}
