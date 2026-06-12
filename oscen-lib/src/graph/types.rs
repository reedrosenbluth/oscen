use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use arrayvec::ArrayVec;

pub const MAX_EVENTS: usize = 256;

/// Default maximum block size for block-based processing.
/// Block buffers on the graph struct are sized to this value.
pub const DEFAULT_MAX_BLOCK_SIZE: usize = 512;
pub const MAX_NODE_ENDPOINTS: usize = 32;
pub const MAX_STREAM_CHANNELS: usize = 128;

/// Maximum number of events per static graph event input/output.
/// This is smaller than MAX_EVENTS to reduce stack usage.
pub const MAX_STATIC_EVENTS_PER_ENDPOINT: usize = 32;

/// Fixed-capacity event queue for static graphs.
/// Uses stack-allocated ArrayVec instead of heap-allocated Vec for zero-overhead event handling.
pub type StaticEventQueue = ArrayVec<EventInstance, MAX_STATIC_EVENTS_PER_ENDPOINT>;

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

#[derive(Clone, Debug)]
pub struct EventInstance {
    pub frame_offset: u32,
    pub payload: EventPayload,
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
    pub fn try_push(
        &mut self,
        event: EventInstance,
    ) -> Result<(), arrayvec::CapacityError<EventInstance>> {
        self.queue.try_push(event)
    }

    /// Get events as a slice (for passing to event handlers).
    pub fn as_slice(&self) -> &[EventInstance] {
        self.queue.as_slice()
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
    pub fn try_push(
        &mut self,
        event: EventInstance,
    ) -> Result<(), arrayvec::CapacityError<EventInstance>> {
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
// SampleRate newtype
// ============================================================================

/// The sample rate (frames per second) a node runs at. Declare a field of this
/// type and `#[derive(Node)]` will fill it automatically from the parent graph
/// (defaulting to 44.1 kHz); there is no need to capture it in `init()`.
///
/// Outside a graph, call the generated `set_sample_rate` method yourself
/// before processing — `prepare()` alone does not fill this field. See the
/// [`SignalProcessor`](crate::graph::SignalProcessor) docs for the full
/// contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleRate(pub f32);

impl Default for SampleRate {
    #[inline]
    fn default() -> Self {
        Self(44100.0)
    }
}

impl SampleRate {
    /// Set the rate. Called by macro-generated code; rarely needed by hand.
    #[inline]
    pub fn set(&mut self, value: f32) {
        self.0 = value;
    }

    /// Seconds per frame — `1.0 / rate`.
    #[inline]
    pub fn period(&self) -> f32 {
        1.0 / self.0
    }

    /// Half the sample rate.
    #[inline]
    pub fn nyquist(&self) -> f32 {
        self.0 * 0.5
    }
}

impl std::ops::Deref for SampleRate {
    type Target = f32;
    #[inline]
    fn deref(&self) -> &f32 {
        &self.0
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

#[cfg(test)]
mod sample_rate_tests {
    use super::SampleRate;

    #[test]
    fn default_is_44100() {
        assert_eq!(*SampleRate::default(), 44100.0);
    }

    #[test]
    fn set_and_read() {
        let mut sr = SampleRate::default();
        sr.set(48_000.0);
        assert_eq!(*sr, 48_000.0);
    }

    #[test]
    fn period_and_nyquist() {
        let sr = SampleRate(48_000.0);
        assert_eq!(sr.period(), 1.0 / 48_000.0);
        assert_eq!(sr.nyquist(), 24_000.0);
    }
}
