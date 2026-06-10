/// Users implement this trait to define their DSP logic. Inputs are already
/// populated in the struct fields by the time process() is called.
///
/// # Sample-rate contract
///
/// The sample rate reaches a node through its [`SampleRate`] field — the
/// *only* way a node learns the rate. Declare `sample_rate: SampleRate` and
/// `#[derive(Node)]` generates a `set_sample_rate` method that fills it.
/// Graphs call `set_sample_rate` on every child **before** calling
/// [`prepare`](Self::prepare), so the field is already correct when
/// `prepare` runs.
///
/// Hosts driving a generated graph use its `init(sample_rate)` method,
/// which distributes the rate and prepares every node in one call.
/// Standalone callers (driving a node without a graph) do the same two
/// steps themselves, in the same order:
///
/// ```text
/// node.set_sample_rate(rate);
/// node.prepare();
/// ```
///
/// Calling `prepare` alone does **not** fill a `SampleRate` field — a node
/// prepared without `set_sample_rate` runs at the 44.1 kHz default.
///
/// [`SampleRate`]: crate::graph::SampleRate
pub trait SignalProcessor: Send + std::fmt::Debug {
    /// Recompute state derived from the sample rate and return the node to
    /// a ready-to-play state — filter coefficients, buffer sizes, envelope
    /// increments. Called by graphs after sample-rate distribution (see the
    /// trait-level docs), before any `process()` call. Read the rate from
    /// your [`SampleRate`](crate::graph::SampleRate) field; nodes whose only
    /// rate-dependent state is that field don't need to implement this at
    /// all.
    ///
    /// Allocation is permitted here (this runs on the control thread, never
    /// the audio thread) — it's the right place to size buffers.
    fn prepare(&mut self) {}

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
