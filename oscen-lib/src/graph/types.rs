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
// Stream/Value Endpoint Types
// ============================================================================

/// A stream input endpoint. Streams carry per-sample audio signals.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StreamInput<T = f32>(pub T);

impl<T: Copy> StreamInput<T> {
    #[inline]
    pub fn set(&mut self, value: T) {
        self.0 = value;
    }
}

impl<T> std::ops::Deref for StreamInput<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

/// Sum an iterator of samples into a `StreamInput`. This lets the graph macro
/// fan an array of stream outputs (e.g. a bank of voices) into a single stream
/// input by summing them, the same way it already does for plain `f32` inputs.
impl std::iter::Sum<f32> for StreamInput {
    #[inline]
    fn sum<I: Iterator<Item = f32>>(iter: I) -> Self {
        StreamInput(iter.sum())
    }
}

/// Fan an iterator of `Frame<N>` stream outputs into a single `StreamInput<Frame<N>>`
/// by summing element-wise — the multi-channel analogue of the `Sum<f32>` impl above.
impl<const N: usize> std::iter::Sum<crate::frame::Frame<N>> for StreamInput<crate::frame::Frame<N>> {
    #[inline]
    fn sum<I: Iterator<Item = crate::frame::Frame<N>>>(iter: I) -> Self {
        StreamInput(iter.sum())
    }
}

/// A stream output endpoint. Streams carry per-sample audio signals.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StreamOutput<T = f32>(pub T);

impl<T> std::ops::Deref for StreamOutput<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for StreamOutput<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// A value input endpoint. Values are control-rate parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ValueInput<T = f32>(pub T);

impl<T: Copy> ValueInput<T> {
    #[inline]
    pub fn set(&mut self, value: T) {
        self.0 = value;
    }
}

impl<T> std::ops::Deref for ValueInput<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

/// A value output endpoint. Values are control-rate parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ValueOutput<T = f32>(pub T);

impl<T> std::ops::Deref for ValueOutput<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for ValueOutput<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

// ============================================================================
// SampleRate newtype
// ============================================================================

/// The sample rate (frames per second) a node runs at. Declare a field of this
/// type and `#[derive(Node)]` will fill it automatically from the parent graph
/// (defaulting to 44.1 kHz); there is no need to capture it in `init()`.
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
// Operator impls (Rust doesn't auto-deref for operators)
// ============================================================================

macro_rules! impl_binops_for {
    ($type:ident) => {
        impl std::ops::Add<f32> for $type<f32> {
            type Output = f32;
            fn add(self, rhs: f32) -> f32 {
                self.0 + rhs
            }
        }
        impl std::ops::Add<$type<f32>> for f32 {
            type Output = f32;
            fn add(self, rhs: $type<f32>) -> f32 {
                self + rhs.0
            }
        }
        impl std::ops::Sub<f32> for $type<f32> {
            type Output = f32;
            fn sub(self, rhs: f32) -> f32 {
                self.0 - rhs
            }
        }
        impl std::ops::Sub<$type<f32>> for f32 {
            type Output = f32;
            fn sub(self, rhs: $type<f32>) -> f32 {
                self - rhs.0
            }
        }
        impl std::ops::Mul<f32> for $type<f32> {
            type Output = f32;
            fn mul(self, rhs: f32) -> f32 {
                self.0 * rhs
            }
        }
        impl std::ops::Mul<$type<f32>> for f32 {
            type Output = f32;
            fn mul(self, rhs: $type<f32>) -> f32 {
                self * rhs.0
            }
        }
        impl std::ops::Div<f32> for $type<f32> {
            type Output = f32;
            fn div(self, rhs: f32) -> f32 {
                self.0 / rhs
            }
        }
        impl std::ops::Div<$type<f32>> for f32 {
            type Output = f32;
            fn div(self, rhs: $type<f32>) -> f32 {
                self / rhs.0
            }
        }
        impl std::ops::Neg for $type<f32> {
            type Output = f32;
            fn neg(self) -> f32 {
                -self.0
            }
        }
        impl PartialEq<f32> for $type<f32> {
            fn eq(&self, other: &f32) -> bool {
                self.0 == *other
            }
        }
        impl PartialOrd<f32> for $type<f32> {
            fn partial_cmp(&self, other: &f32) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(other)
            }
        }
    };
}

macro_rules! impl_cross_binops {
    ($lhs:ident, $rhs:ident) => {
        impl std::ops::Add<$rhs<f32>> for $lhs<f32> {
            type Output = f32;
            fn add(self, rhs: $rhs<f32>) -> f32 {
                self.0 + rhs.0
            }
        }
        impl std::ops::Sub<$rhs<f32>> for $lhs<f32> {
            type Output = f32;
            fn sub(self, rhs: $rhs<f32>) -> f32 {
                self.0 - rhs.0
            }
        }
        impl std::ops::Mul<$rhs<f32>> for $lhs<f32> {
            type Output = f32;
            fn mul(self, rhs: $rhs<f32>) -> f32 {
                self.0 * rhs.0
            }
        }
        impl std::ops::Div<$rhs<f32>> for $lhs<f32> {
            type Output = f32;
            fn div(self, rhs: $rhs<f32>) -> f32 {
                self.0 / rhs.0
            }
        }
    };
}

impl_binops_for!(StreamInput);
impl_binops_for!(StreamOutput);
impl_binops_for!(ValueInput);
impl_binops_for!(ValueOutput);

// Self-type ops (Type op Type)
impl_cross_binops!(StreamInput, StreamInput);
impl_cross_binops!(StreamOutput, StreamOutput);
impl_cross_binops!(ValueInput, ValueInput);
impl_cross_binops!(ValueOutput, ValueOutput);

// Cross-type ops (Stream op Value and vice versa)
impl_cross_binops!(StreamInput, ValueInput);
impl_cross_binops!(ValueInput, StreamInput);
impl_cross_binops!(StreamOutput, ValueInput);
impl_cross_binops!(ValueInput, StreamOutput);
impl_cross_binops!(StreamInput, ValueOutput);
impl_cross_binops!(StreamInput, StreamOutput);
impl_cross_binops!(StreamOutput, StreamInput);

// Sum trait for iterators (needed for array-to-output summing in graph macro)
impl std::iter::Sum for StreamOutput<f32> {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        StreamOutput(iter.map(|x| x.0).sum())
    }
}

impl std::iter::Sum<StreamOutput<f32>> for f32 {
    fn sum<I: Iterator<Item = StreamOutput<f32>>>(iter: I) -> f32 {
        iter.map(|x| x.0).sum()
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

    // ------------------------------------------------------------------
    // Sum impls used by the graph macro's array-to-scalar fan-in.
    // The macro lowers `array.field -> dest.field` to
    // `dest.field = array.iter().map(|n| n.field).sum()`, so the element
    // type and destination type pick which Sum impl is selected.
    // ------------------------------------------------------------------

    #[test]
    fn stream_input_sums_f32_iterator() {
        // Raw-f32 element outputs fanning into a typed `StreamInput` dest.
        let summed: StreamInput = [1.0_f32, 2.0, 3.0, 4.0].into_iter().sum();
        assert_eq!(summed.0, 10.0);
    }

    #[test]
    fn stream_input_sum_of_empty_is_zero() {
        // Fan-in identity: an empty source array must yield silence, not NaN.
        let summed: StreamInput = std::iter::empty::<f32>().sum();
        assert_eq!(summed.0, 0.0);
    }

    #[test]
    fn stream_input_sum_single_element_is_passthrough() {
        let summed: StreamInput = std::iter::once(0.5_f32).sum();
        assert_eq!(summed.0, 0.5);
    }

    #[test]
    fn stream_output_sums_into_stream_output() {
        // Sibling impl: StreamOutput elements summed into a StreamOutput.
        let summed: StreamOutput<f32> =
            [StreamOutput(1.5_f32), StreamOutput(2.5)].into_iter().sum();
        assert_eq!(summed.0, 4.0);
    }

    #[test]
    fn stream_output_sums_into_f32() {
        // Sibling impl: StreamOutput elements summed into a bare f32 dest
        // (e.g. a `#[output(stream)] f32` graph output or node input).
        let summed: f32 = [StreamOutput(1.0_f32), StreamOutput(2.0), StreamOutput(3.0)]
            .into_iter()
            .sum();
        assert_eq!(summed, 6.0);
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
