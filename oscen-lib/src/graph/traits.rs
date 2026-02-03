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

    /// Whether this node can break feedback cycles (e.g., delay lines).
    #[inline]
    fn allows_feedback(&self) -> bool {
        false
    }

    /// Returns whether this node is currently active and producing meaningful output.
    /// Inactive nodes can be skipped during processing, with their outputs set to 0.0.
    #[inline]
    fn is_active(&self) -> bool {
        true
    }
}
