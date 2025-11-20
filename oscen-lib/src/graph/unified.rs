/// Unified interface for both static and runtime graphs.
/// This trait provides a common API that works regardless of `compile_time` setting,
/// enabling seamless switching between modes for performance comparison.
///
/// # Example
/// ```ignore
/// fn run_synth<G: GraphInterface>(mut graph: G) {
///     graph.set_input_value("cutoff", 1000.0);
///     for _ in 0..480 {
///         let sample = graph.process_sample();
///         // Output sample...
///     }
/// }
///
/// // Works with either mode:
/// run_synth(StaticGraph::new(48000.0));  // compile_time: true
/// run_synth(RuntimeGraph::new(48000.0)); // compile_time: false
/// ```
pub trait GraphInterface {
    /// Process one sample and return the primary output
    fn process_sample(&mut self) -> f32;

    /// Set an input value by name
    fn set_input_value(&mut self, name: &str, value: f32);

    /// Get an output value by name
    fn get_output_value(&self, name: &str) -> f32;

    /// Get the sample rate
    fn sample_rate(&self) -> f32;
}
