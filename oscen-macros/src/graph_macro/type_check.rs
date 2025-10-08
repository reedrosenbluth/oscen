use super::ast::*;
use std::collections::HashMap;
use syn::{Ident, Result};

/// Tracks the inferred types of expressions in the graph
pub struct TypeContext {
    /// Known types for inputs
    inputs: HashMap<String, EndpointKind>,
    /// Known types for outputs
    outputs: HashMap<String, EndpointKind>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            inputs: HashMap::new(),
            outputs: HashMap::new(),
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

    /// Infer the type of a connection expression
    pub fn infer_type(&self, expr: &ConnectionExpr) -> Option<EndpointKind> {
        match expr {
            ConnectionExpr::Ident(ident) => {
                let name = ident.to_string();
                // Check if it's a known input or output
                self.inputs.get(&name).or_else(|| self.outputs.get(&name)).copied()
            }
            ConnectionExpr::ArrayIndex(array_expr, _idx) => {
                // Array indexing preserves the type of the base expression
                self.infer_type(array_expr)
            }
            ConnectionExpr::Method(obj, method, _args) => {
                // Try to infer based on common method names
                let method_name = method.to_string();

                // Common output methods (return StreamOutput)
                if method_name == "output" {
                    return Some(EndpointKind::Stream);
                }

                // Common input methods (return various input types)
                // These are heuristics based on oscen's API patterns
                match method_name.as_str() {
                    // Stream inputs
                    "input" | "audio_in" | "signal_in" => Some(EndpointKind::Stream),

                    // Value inputs (control parameters)
                    "frequency" | "amplitude" | "cutoff" | "q" | "resonance"
                    | "attack" | "decay" | "sustain" | "release"
                    | "f_mod" | "q_mod" => Some(EndpointKind::Value),

                    // Event inputs
                    "gate" | "trigger" | "note_on" | "note_off" => Some(EndpointKind::Event),

                    // Unknown method
                    _ => None,
                }
            }
            ConnectionExpr::Binary(left, op, right) => {
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
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        msg,
                    ));
                }
            }
            // If we can't infer types, let Rust's type system handle it
            _ => {}
        }

        Ok(())
    }

    /// Validate that a source expression can be used as an output
    pub fn validate_source(&self, expr: &ConnectionExpr) -> Result<()> {
        // Sources should produce outputs (not inputs)
        match expr {
            ConnectionExpr::Method(_, method, _) => {
                let method_name = method.to_string();

                // Check for common input-only methods being used as sources
                // Note: Some methods like "frequency" can be both inputs and outputs
                // depending on the node, so we only list truly input-only methods here
                let input_methods = [
                    "input", "amplitude", "cutoff", "q",
                    "trigger", "attack", "decay", "sustain", "release",
                    "f_mod", "q_mod",
                ];

                if input_methods.contains(&method_name.as_str()) {
                    return Err(syn::Error::new_spanned(
                        method,
                        format!(
                            "Method '{}' is an input endpoint and cannot be used as a source in a connection. Did you mean to use this as the destination?",
                            method_name
                        ),
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Validate that a destination expression can receive input
    pub fn validate_destination(&self, expr: &ConnectionExpr) -> Result<()> {
        // Destinations should be inputs
        match expr {
            ConnectionExpr::Method(_, method, _) => {
                let method_name = method.to_string();

                // Check for output methods being used as destinations
                if method_name == "output" {
                    return Err(syn::Error::new_spanned(
                        method,
                        "Method 'output' produces an output endpoint and cannot be used as a destination in a connection. Did you mean to use this as the source?",
                    ));
                }
            }
            ConnectionExpr::Ident(ident) => {
                // Check if it's a declared output
                if self.outputs.contains_key(&ident.to_string()) {
                    // This is OK - outputs can be destinations for final graph outputs
                    return Ok(());
                }
            }
            _ => {}
        }

        Ok(())
    }
}
