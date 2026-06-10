/// Users implement this trait to define their DSP logic. Inputs are already
/// populated in the struct fields by the time process() is called.
///
/// # Sample-rate contract
///
/// The sample rate reaches a node through its [`SampleRate`] field: declare
/// `sample_rate: SampleRate` and `#[derive(Node)]` generates a
/// `set_sample_rate` method that fills it. Graphs call `set_sample_rate`
/// on every child **before** calling [`init`](Self::init), so the field is
/// already correct when `init` runs.
///
/// Standalone callers (driving a node without a graph) must follow the same
/// order:
///
/// ```text
/// node.set_sample_rate(rate);
/// node.init(rate);
/// ```
///
/// Calling `init` alone does **not** fill a `SampleRate` field — nodes that
/// rely on the field for their per-sample math will keep the 44.1 kHz
/// default unless `set_sample_rate` is called.
///
/// [`SampleRate`]: crate::graph::SampleRate
pub trait SignalProcessor: Send + std::fmt::Debug {
    /// Called once by the graph after sample-rate distribution (see the
    /// trait-level docs), before any `process()` call. Implement it to
    /// compute state derived from the sample rate — filter coefficients,
    /// buffer sizes, envelope increments. Nodes whose only rate-dependent
    /// state is a [`SampleRate`](crate::graph::SampleRate) field don't need
    /// to implement it at all.
    fn init(&mut self, _sample_rate: f32) {}

    /// Process one sample of audio.
    ///
    /// All inputs are already populated in struct fields. Write outputs to
    /// output fields. No context object to deal with!
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
