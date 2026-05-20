/// Users implement this trait to define their DSP logic. Inputs are already
/// populated in the struct fields by the time process() is called.
pub trait SignalProcessor: Send + std::fmt::Debug {
    /// Called once when the node is added to a graph.
    fn init(&mut self, _sample_rate: f32) {}

    /// Process one sample of audio.
    ///
    /// All inputs are already populated in struct fields. Write outputs to
    /// output fields. No context object to deal with!
    ///
    /// Sample rate is stored in the node during init() or construction.
    fn process(&mut self);

    /// Returns whether this node is currently active and producing meaningful output.
    /// Inactive nodes can be skipped during processing, with their outputs set to 0.0.
    #[inline]
    fn is_active(&self) -> bool {
        true
    }
}

/// Marker trait for nodes that can sit inside a feedback cycle in a `graph!`
/// macro. The macro emits a `T: AllowsFeedback` static assertion for each node
/// it picks as a cycle-breaker, so a type that the macro treats as feedback-
/// allowing must implement this trait or the generated code fails to compile.
///
/// The library implements this for [`crate::delay::Delay`] only. Custom types
/// that introduce an explicit one-sample delay (and therefore safely break
/// cycles) can opt in by adding their own `impl AllowsFeedback for MyType {}`.
pub trait AllowsFeedback: SignalProcessor {}
