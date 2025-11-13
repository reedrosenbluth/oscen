use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::Node;

/// A simple passthrough node that forwards events from input to output.
/// This is used internally by the graph macro to enable graph-level event inputs
/// that can both receive events (via queue_event) and route them to other nodes.
#[derive(Debug, Node)]
pub struct EventPassthrough {
    #[input(event)]
    input: (),

    #[output(event)]
    output: (),
}

impl EventPassthrough {
    pub fn new() -> Self {
        Self {
            input: (),
            output: (),
        }
    }
}

impl Default for EventPassthrough {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for EventPassthrough {
    fn process(&mut self, _sample_rate: f32) {
        // All event processing is done via on_input handler
        // This node has no stream outputs to update
    }
}

impl EventPassthrough {
    // Event handler called automatically by macro-generated NodeIO
    fn on_input(&mut self, event: &crate::graph::EventInstance, context: &mut ProcessingContext) {
        // Forward event to output (output index 0)
        context.emit_event(0, event.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EventPayload;
    use crate::Graph;

    #[test]
    fn test_event_passthrough() {
        let mut graph = Graph::new(44100.0);
        let passthrough = graph.add_node(EventPassthrough::new());

        // Queue an event to the passthrough input
        assert!(graph.queue_event(passthrough.input, 0, EventPayload::scalar(1.0)));

        // Process the graph
        graph.process().expect("graph processes");

        // Verify event was forwarded to output
        let mut event_count = 0;
        graph.drain_events(passthrough.output, |event| {
            assert_eq!(event.frame_offset, 0);
            match event.payload {
                EventPayload::Scalar(v) => assert_eq!(v, 1.0),
                _ => panic!("expected scalar payload"),
            }
            event_count += 1;
        });

        assert_eq!(event_count, 1, "event should be forwarded");
    }

    #[test]
    fn test_multiple_events() {
        let mut graph = Graph::new(44100.0);
        let passthrough = graph.add_node(EventPassthrough::new());

        // Queue multiple events
        assert!(graph.queue_event(passthrough.input, 0, EventPayload::scalar(1.0)));
        assert!(graph.queue_event(passthrough.input, 10, EventPayload::scalar(2.0)));
        assert!(graph.queue_event(passthrough.input, 20, EventPayload::scalar(3.0)));

        // Process the graph
        graph.process().expect("graph processes");

        // Verify all events were forwarded
        let mut events = Vec::new();
        graph.drain_events(passthrough.output, |event| {
            events.push(event.clone());
        });

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].frame_offset, 0);
        assert_eq!(events[1].frame_offset, 10);
        assert_eq!(events[2].frame_offset, 20);

        match events[0].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 1.0),
            _ => panic!("expected scalar payload"),
        }
        match events[1].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 2.0),
            _ => panic!("expected scalar payload"),
        }
        match events[2].payload {
            EventPayload::Scalar(v) => assert_eq!(v, 3.0),
            _ => panic!("expected scalar payload"),
        }
    }
}
