use arrayvec::ArrayVec;

use super::types::NodeKey;
use super::types::{
    EndpointType, EventInstance, EventPayload, ValueData, ValueKey, ValueObject, MAX_NODE_ENDPOINTS,
};

#[derive(Copy, Clone)]
pub struct ValueRef<'a> {
    data: &'a ValueData,
}

impl<'a> ValueRef<'a> {
    pub(crate) fn new(data: &'a ValueData) -> Self {
        Self { data }
    }

    pub fn as_scalar(&self) -> Option<f32> {
        self.data.as_scalar()
    }

    pub fn as_object(&self) -> Option<&'a dyn ValueObject> {
        self.data.as_object()
    }

    pub fn data(&self) -> &'a ValueData {
        self.data
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub output_index: usize,
    pub event: EventInstance,
}

pub struct ProcessingContext<'a> {
    scalar_inputs: &'a [f32],
    value_inputs: &'a [Option<&'a ValueData>],
    event_inputs: &'a [&'a [EventInstance]],
    emitted_events: &'a mut Vec<PendingEvent>,
    frame_index: usize,
    block_length: usize,
}

impl<'a> ProcessingContext<'a> {
    pub(crate) fn new(
        scalar_inputs: &'a [f32],
        value_inputs: &'a [Option<&'a ValueData>],
        event_inputs: &'a [&'a [EventInstance]],
        emitted_events: &'a mut Vec<PendingEvent>,
        frame_index: usize,
        block_length: usize,
    ) -> Self {
        Self {
            scalar_inputs,
            value_inputs,
            event_inputs,
            emitted_events,
            frame_index,
            block_length,
        }
    }

    pub fn stream(&self, index: usize) -> f32 {
        self.scalar_inputs.get(index).copied().unwrap_or(0.0)
    }

    pub fn value(&self, index: usize) -> Option<ValueRef<'a>> {
        self.value_inputs
            .get(index)
            .and_then(|opt| opt.map(ValueRef::new))
    }

    pub fn value_scalar(&self, index: usize) -> f32 {
        self.value(index)
            .and_then(|value| value.as_scalar())
            .unwrap_or_else(|| self.stream(index))
    }

    pub fn events(&self, index: usize) -> &[EventInstance] {
        self.event_inputs.get(index).copied().unwrap_or_default()
    }

    pub fn emit_event(&mut self, output_index: usize, event: EventInstance) {
        self.emitted_events.push(PendingEvent {
            output_index,
            event,
        });
    }

    pub fn emit_timed_event(
        &mut self,
        output_index: usize,
        frame_offset: u32,
        payload: EventPayload,
    ) {
        self.emit_event(
            output_index,
            EventInstance {
                frame_offset,
                payload,
            },
        );
    }

    pub fn emit_scalar_event(&mut self, output_index: usize, frame_offset: u32, payload: f32) {
        self.emit_timed_event(output_index, frame_offset, EventPayload::scalar(payload));
    }

    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    pub fn block_length(&self) -> usize {
        self.block_length
    }
}

pub trait SignalProcessor: Send + std::fmt::Debug {
    fn init(&mut self, _sample_rate: f32) {}
    fn process<'a>(&mut self, sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32;

    fn allows_feedback(&self) -> bool {
        false
    }
}

pub trait ProcessingNode: SignalProcessor {
    type Endpoints;

    const INPUT_TYPES: &'static [EndpointType] = &[];
    const OUTPUT_TYPES: &'static [EndpointType] = &[];

    fn create_endpoints(
        node_key: NodeKey,
        inputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
        outputs: ArrayVec<ValueKey, MAX_NODE_ENDPOINTS>,
    ) -> Self::Endpoints;
}
