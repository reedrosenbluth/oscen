use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use arrayvec::ArrayVec;

pub const MAX_EVENTS: usize = 256;
pub const MAX_NODE_ENDPOINTS: usize = 32;
pub const MAX_STREAM_CHANNELS: usize = 128;

/// Maximum number of events per static graph event input/output.
/// This is smaller than MAX_EVENTS to reduce stack usage.
pub const MAX_STATIC_EVENTS_PER_ENDPOINT: usize = 32;

/// Fixed-capacity event queue for static graphs.
/// Uses stack-allocated ArrayVec instead of heap-allocated Vec for zero-overhead event handling.
pub type StaticEventQueue = ArrayVec<EventInstance, MAX_STATIC_EVENTS_PER_ENDPOINT>;

/// Opaque key type for node identification.
/// Used by the derive macro for endpoint generation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodeKey(());

/// Opaque key type for value/endpoint identification.
/// Used by the derive macro for endpoint generation.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ValueKey(());

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointType {
    Stream,
    Value,
    Event,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndpointDirection {
    Input,
    Output,
}

#[derive(Clone, Debug, Default)]
pub struct EndpointAnnotations {
    // Placeholder for future UI/validation metadata.
}

#[derive(Clone, Debug)]
pub struct EndpointDescriptor {
    pub name: &'static str,
    pub endpoint_type: EndpointType,
    pub direction: EndpointDirection,
    pub annotations: EndpointAnnotations,
}

impl EndpointDescriptor {
    pub const fn new(
        name: &'static str,
        endpoint_type: EndpointType,
        direction: EndpointDirection,
    ) -> Self {
        Self {
            name,
            endpoint_type,
            direction,
            annotations: EndpointAnnotations {},
        }
    }
}

pub trait ValueObject: Send + Sync + 'static + fmt::Debug {}

impl<T> ValueObject for T where T: Send + Sync + 'static + fmt::Debug {}

pub trait EventObject: Send + Sync + 'static + fmt::Debug {
    fn as_any(&self) -> &dyn Any;
}

impl<T> EventObject for T
where
    T: Send + Sync + 'static + fmt::Debug,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
pub enum ValueData {
    Scalar(f32),
    Object(Arc<dyn ValueObject>),
}

impl fmt::Debug for ValueData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(v) => f.debug_tuple("Scalar").field(v).finish(),
            Self::Object(obj) => f.debug_tuple("Object").field(obj).finish(),
        }
    }
}

impl ValueData {
    pub fn scalar(value: f32) -> Self {
        Self::Scalar(value)
    }

    pub fn object<T>(value: T) -> Self
    where
        T: ValueObject,
    {
        Self::Object(Arc::new(value))
    }

    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Scalar(v) => Some(*v),
            Self::Object(_) => None,
        }
    }

    pub fn as_scalar_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Scalar(v) => Some(v),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&dyn ValueObject> {
        match self {
            Self::Scalar(_) => None,
            Self::Object(obj) => Some(obj.as_ref()),
        }
    }

    pub fn set_scalar(&mut self, value: f32) {
        if let Some(slot) = self.as_scalar_mut() {
            *slot = value;
        } else {
            *self = Self::Scalar(value);
        }
    }
}

#[derive(Clone)]
pub enum EventPayload {
    Scalar(f32),
    Object(Arc<dyn EventObject>),
}

impl fmt::Debug for EventPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(v) => f.debug_tuple("Scalar").field(v).finish(),
            Self::Object(obj) => f.debug_tuple("Object").field(obj).finish(),
        }
    }
}

impl EventPayload {
    pub fn scalar(value: f32) -> Self {
        Self::Scalar(value)
    }

    pub fn object<T>(value: T) -> Self
    where
        T: EventObject,
    {
        Self::Object(Arc::new(value))
    }

    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Scalar(v) => Some(*v),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&dyn EventObject> {
        match self {
            Self::Scalar(_) => None,
            Self::Object(obj) => Some(obj.as_ref()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventEndpointState {
    queue: EventQueue,
}

impl EventEndpointState {
    pub fn new(max_events: usize) -> Self {
        Self {
            queue: EventQueue::new(max_events),
        }
    }

    pub fn queue(&self) -> &EventQueue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut EventQueue {
        &mut self.queue
    }
}

#[derive(Debug)]
pub enum EndpointState {
    Stream(ArrayVec<f32, MAX_STREAM_CHANNELS>),
    Value(ValueData),
    Event(EventEndpointState),
}

impl EndpointState {
    pub fn stream(initial: f32) -> Self {
        let mut channels = ArrayVec::new();
        channels.push(initial);
        Self::Stream(channels)
    }

    pub fn value(initial: f32) -> Self {
        Self::Value(ValueData::scalar(initial))
    }

    pub fn event() -> Self {
        Self::Event(EventEndpointState::new(MAX_EVENTS))
    }

    #[inline]
    pub fn as_scalar(&self) -> Option<f32> {
        match self {
            Self::Stream(channels) => channels.first().copied(),
            Self::Value(data) => data.as_scalar(),
            Self::Event(_) => None,
        }
    }

    #[inline]
    pub fn as_scalar_mut(&mut self) -> Option<&mut f32> {
        match self {
            Self::Stream(channels) => channels.first_mut(),
            Self::Value(data) => data.as_scalar_mut(),
            Self::Event(_) => None,
        }
    }

    #[inline]
    pub fn set_scalar(&mut self, value: f32) {
        match self {
            Self::Stream(channels) => {
                channels.clear();
                channels.push(value);
            }
            Self::Value(data) => data.set_scalar(value),
            Self::Event(_) => {}
        }
    }

    #[inline]
    pub fn as_channels(&self) -> Option<&[f32]> {
        match self {
            Self::Stream(channels) => Some(channels.as_slice()),
            _ => None,
        }
    }

    #[inline]
    pub fn as_channels_mut(&mut self) -> Option<&mut ArrayVec<f32, MAX_STREAM_CHANNELS>> {
        match self {
            Self::Stream(channels) => Some(channels),
            _ => None,
        }
    }

    #[inline]
    pub fn set_channels(&mut self, values: &[f32]) {
        if let Self::Stream(channels) = self {
            channels.clear();
            channels.try_extend_from_slice(values).ok();
        }
    }

    #[inline]
    pub fn as_event(&self) -> Option<&EventEndpointState> {
        match self {
            Self::Event(state) => Some(state),
            _ => None,
        }
    }

    #[inline]
    pub fn as_event_mut(&mut self) -> Option<&mut EventEndpointState> {
        match self {
            Self::Event(state) => Some(state),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventInstance {
    pub frame_offset: u32,
    pub payload: EventPayload,
}

#[derive(Debug, Clone)]
pub struct EventQueue {
    events: Vec<EventInstance>,
    max_events: usize,
}

impl EventQueue {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Vec::with_capacity(max_events),
            max_events,
        }
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn push(&mut self, event: EventInstance) -> bool {
        if self.events.len() < self.max_events {
            self.events.push(event);
            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &EventInstance> {
        self.events.iter()
    }

    pub fn events(&self) -> &[EventInstance] {
        &self.events
    }

    pub fn events_mut(&mut self) -> &mut Vec<EventInstance> {
        &mut self.events
    }

    pub fn max_events(&self) -> usize {
        self.max_events
    }
}

#[derive(Copy, Clone, Debug)]
pub struct InputEndpoint {
    key: ValueKey,
}

impl Default for InputEndpoint {
    fn default() -> Self {
        Self {
            key: ValueKey::default(),
        }
    }
}

impl InputEndpoint {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

// ============================================================================
// Typed Input Handles
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct ValueInput {
    endpoint: InputEndpoint,
}

impl ValueInput {
    pub fn new(endpoint: InputEndpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> InputEndpoint {
        self.endpoint
    }

    pub fn key(&self) -> ValueKey {
        self.endpoint.key()
    }
}

impl From<ValueInput> for ValueKey {
    fn from(handle: ValueInput) -> Self {
        handle.key()
    }
}

impl From<&ValueInput> for ValueKey {
    fn from(handle: &ValueInput) -> Self {
        handle.key()
    }
}

impl From<ValueInput> for InputEndpoint {
    fn from(handle: ValueInput) -> Self {
        handle.endpoint()
    }
}

impl From<&ValueInput> for InputEndpoint {
    fn from(handle: &ValueInput) -> Self {
        handle.endpoint()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct StreamInput {
    endpoint: InputEndpoint,
}

impl StreamInput {
    pub fn new(endpoint: InputEndpoint) -> Self {
        Self { endpoint }
    }

    pub fn endpoint(&self) -> InputEndpoint {
        self.endpoint
    }

    pub fn key(&self) -> ValueKey {
        self.endpoint.key()
    }
}

impl From<StreamInput> for ValueKey {
    fn from(handle: StreamInput) -> Self {
        handle.key()
    }
}

impl From<&StreamInput> for ValueKey {
    fn from(handle: &StreamInput) -> Self {
        handle.key()
    }
}

impl From<StreamInput> for InputEndpoint {
    fn from(handle: StreamInput) -> Self {
        handle.endpoint()
    }
}

impl From<&StreamInput> for InputEndpoint {
    fn from(handle: &StreamInput) -> Self {
        handle.endpoint()
    }
}

/// Event input endpoint with built-in storage.
/// Contains a StaticEventQueue for storing incoming events.
#[derive(Debug, Clone)]
pub struct EventInput<T = EventPayload> {
    queue: StaticEventQueue,
    _marker: PhantomData<T>,
}

impl<T> Default for EventInput<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EventInput<T> {
    /// Create a new empty event input.
    pub fn new() -> Self {
        Self {
            queue: StaticEventQueue::new(),
            _marker: PhantomData,
        }
    }

    /// Iterate over events in this input.
    pub fn iter(&self) -> impl Iterator<Item = &EventInstance> {
        self.queue.iter()
    }

    /// Clear all events from this input.
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Get the number of events in this input.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Check if this input has no events.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Try to push an event into this input.
    pub fn try_push(&mut self, event: EventInstance) -> Result<(), arrayvec::CapacityError<EventInstance>> {
        self.queue.try_push(event)
    }

    /// Get events as a slice (for passing to event handlers).
    pub fn as_slice(&self) -> &[EventInstance] {
        self.queue.as_slice()
    }
}

// ============================================================================
// Typed Output Handles
// ============================================================================

/// Trait for output endpoints - allows generic functions over all output types
pub trait Output {
    fn key(&self) -> ValueKey;
}

#[derive(Copy, Clone, Debug)]
pub struct StreamOutput {
    key: ValueKey,
}

impl StreamOutput {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl Output for StreamOutput {
    fn key(&self) -> ValueKey {
        self.key
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ValueOutput {
    key: ValueKey,
}

impl ValueOutput {
    pub fn new(key: ValueKey) -> Self {
        Self { key }
    }

    pub fn key(&self) -> ValueKey {
        self.key
    }
}

impl Output for ValueOutput {
    fn key(&self) -> ValueKey {
        self.key
    }
}

/// Event output endpoint with built-in storage.
/// Contains a StaticEventQueue for storing outgoing events.
#[derive(Debug, Clone)]
pub struct EventOutput<T = EventPayload> {
    queue: StaticEventQueue,
    _marker: PhantomData<T>,
}

impl<T> Default for EventOutput<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> EventOutput<T> {
    /// Create a new empty event output.
    pub fn new() -> Self {
        Self {
            queue: StaticEventQueue::new(),
            _marker: PhantomData,
        }
    }

    /// Try to push an event into this output.
    pub fn try_push(&mut self, event: EventInstance) -> Result<(), arrayvec::CapacityError<EventInstance>> {
        self.queue.try_push(event)
    }

    /// Iterate over events in this output.
    pub fn iter(&self) -> impl Iterator<Item = &EventInstance> {
        self.queue.iter()
    }

    /// Clear all events from this output.
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Get the number of events in this output.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Check if this output has no events.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// ============================================================================
// ValueParam - Opaque parameter handle
// ============================================================================

/// An opaque handle to a value parameter that can be both updated and connected.
/// Created by `Graph::value_param()`.
#[derive(Copy, Clone, Debug)]
pub struct ValueParam {
    pub(crate) input: ValueInput,
    pub(crate) output: ValueOutput,
}

impl ValueParam {
    pub fn new(input: ValueInput, output: ValueOutput) -> Self {
        Self { input, output }
    }
}

impl Output for ValueParam {
    fn key(&self) -> ValueKey {
        self.output.key()
    }
}

// Allow ValueParam to be used where ValueKey is expected (for set_value, set_value_with_ramp, etc)
impl From<ValueParam> for ValueKey {
    fn from(param: ValueParam) -> Self {
        param.input.key()
    }
}

impl From<&ValueParam> for ValueKey {
    fn from(param: &ValueParam) -> Self {
        param.input.key()
    }
}

// ============================================================================
// ValueRampState - Linear interpolation for value inputs
// ============================================================================

/// State for linear value interpolation.
/// Used for smooth parameter transitions to avoid audio artifacts.
#[derive(Debug, Clone, Copy)]
pub struct ValueRampState {
    pub current: f32,
    pub target: f32,
    increment: f32,
    frames_remaining: u32,
}

impl Default for ValueRampState {
    fn default() -> Self {
        Self {
            current: 0.0,
            target: 0.0,
            increment: 0.0,
            frames_remaining: 0,
        }
    }
}

impl ValueRampState {
    /// Create a new ValueRampState with an initial value.
    pub fn new(initial: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            increment: 0.0,
            frames_remaining: 0,
        }
    }

    /// Set the value immediately without ramping.
    #[inline]
    pub fn set_immediate(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.increment = 0.0;
        self.frames_remaining = 0;
    }

    /// Set the target value with a ramp over the specified number of frames.
    #[inline]
    pub fn set_with_ramp(&mut self, target: f32, frames: u32) {
        if frames == 0 {
            self.set_immediate(target);
        } else {
            self.target = target;
            self.increment = (target - self.current) / frames as f32;
            self.frames_remaining = frames;
        }
    }

    /// Advance the interpolation by one frame.
    /// Call this once per sample before using the `current` value.
    /// Returns `true` if the ramp just completed (for decrementing active_ramps counter).
    #[inline]
    pub fn tick(&mut self) -> bool {
        if self.frames_remaining > 0 {
            self.frames_remaining -= 1;
            if self.frames_remaining == 0 {
                self.current = self.target;
                self.increment = 0.0;
                return true; // Ramp completed
            } else {
                self.current += self.increment;
            }
        }
        false
    }

    /// Returns true if the ramp is currently active.
    #[inline]
    pub fn is_ramping(&self) -> bool {
        self.frames_remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_ramp_state_new() {
        let ramp = ValueRampState::new(100.0);
        assert_eq!(ramp.current, 100.0);
        assert_eq!(ramp.target, 100.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_set_immediate() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_immediate(50.0);
        assert_eq!(ramp.current, 50.0);
        assert_eq!(ramp.target, 50.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_set_with_ramp_zero_frames() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_with_ramp(100.0, 0);
        assert_eq!(ramp.current, 100.0);
        assert_eq!(ramp.target, 100.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_set_with_ramp() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_with_ramp(100.0, 10);
        assert_eq!(ramp.current, 0.0);
        assert_eq!(ramp.target, 100.0);
        assert!(ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_tick_advances_correctly() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_with_ramp(100.0, 4);

        // Tick 4 times
        assert!(!ramp.tick()); // Not completed yet
        assert!((ramp.current - 25.0).abs() < 0.001);
        assert!(ramp.is_ramping());

        assert!(!ramp.tick()); // Not completed yet
        assert!((ramp.current - 50.0).abs() < 0.001);
        assert!(ramp.is_ramping());

        assert!(!ramp.tick()); // Not completed yet
        assert!((ramp.current - 75.0).abs() < 0.001);
        assert!(ramp.is_ramping());

        assert!(ramp.tick()); // Completed!
        // Should land exactly on target
        assert_eq!(ramp.current, 100.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_tick_does_nothing_when_not_ramping() {
        let mut ramp = ValueRampState::new(42.0);
        assert!(!ramp.tick()); // Returns false when not ramping
        assert_eq!(ramp.current, 42.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_lands_on_target() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_with_ramp(1.0, 100);

        for i in 0..100 {
            let completed = ramp.tick();
            // Only the last tick should return true
            assert_eq!(completed, i == 99);
        }

        // Should be exactly on target, not accumulated floating point error
        assert_eq!(ramp.current, 1.0);
        assert!(!ramp.is_ramping());
    }

    #[test]
    fn value_ramp_state_downward_ramp() {
        let mut ramp = ValueRampState::new(100.0);
        ramp.set_with_ramp(0.0, 4);

        assert!(!ramp.tick());
        assert!((ramp.current - 75.0).abs() < 0.001);

        assert!(!ramp.tick());
        assert!((ramp.current - 50.0).abs() < 0.001);

        assert!(!ramp.tick());
        assert!((ramp.current - 25.0).abs() < 0.001);

        assert!(ramp.tick()); // Completed!
        assert_eq!(ramp.current, 0.0);
    }

    #[test]
    fn value_ramp_state_interrupt_ramp() {
        let mut ramp = ValueRampState::new(0.0);
        ramp.set_with_ramp(100.0, 10);

        // Tick a few times
        assert!(!ramp.tick());
        assert!(!ramp.tick());
        assert!(ramp.is_ramping());

        // Interrupt with new ramp from current position
        let current = ramp.current;
        ramp.set_with_ramp(0.0, 4);

        // Should ramp from the interrupted position
        assert!(ramp.is_ramping());
        assert_eq!(ramp.current, current);

        for i in 0..4 {
            let completed = ramp.tick();
            assert_eq!(completed, i == 3);
        }
        assert_eq!(ramp.current, 0.0);
    }
}

