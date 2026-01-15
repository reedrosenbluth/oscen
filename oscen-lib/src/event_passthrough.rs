use crate::graph::{
    EventContext, EventInput, EventOutput, InputEndpoint, NodeKey, ProcessingNode, SignalProcessor,
    ValueKey,
};
use crate::Node;

/// A simple passthrough node that forwards events from input to output.
/// This is used internally by the graph macro to enable graph-level event inputs
/// that can both receive events (via queue_event) and route them to other nodes.
#[derive(Debug, Node)]
pub struct EventPassthrough {
    #[input(event)]
    input: EventInput,

    #[output(event)]
    output: EventOutput,
}

impl EventPassthrough {
    pub fn new() -> Self {
        Self {
            input: EventInput::default(),
            output: EventOutput::default(),
        }
    }
}

impl Default for EventPassthrough {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for EventPassthrough {
    fn process(&mut self) {
        // All event processing is done via on_input handler
        // This node has no stream outputs to update
    }
}

impl EventPassthrough {
    // Event handler called automatically by macro-generated NodeIO
    fn on_input(&mut self, event: &crate::graph::EventInstance, ctx: &mut impl EventContext) {
        // Forward event to output (output index 0)
        ctx.emit_event(0, event.clone());
    }
}

// Note: Tests for EventPassthrough are integration tests using static graphs
// The runtime Graph tests have been removed
