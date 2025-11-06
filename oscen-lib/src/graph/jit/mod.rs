/// JIT compilation support for graph execution
///
/// This module provides JIT compilation of graph execution using Cranelift.
/// The JIT compiler generates optimized machine code for the graph topology,
/// eliminating interpreted loops and SlotMap lookups.
///
/// Key design principle: Node authors don't need to change anything!
/// We JIT compile the graph execution loop, but still call into existing
/// node implementations via function pointers.

pub mod ir;
pub mod compiler;
pub mod runtime;

pub use ir::GraphIR;
pub use compiler::{CraneliftJit, CompiledGraph};
pub use runtime::{GraphState, GraphStateBuilder};

#[cfg(test)]
mod tests;
