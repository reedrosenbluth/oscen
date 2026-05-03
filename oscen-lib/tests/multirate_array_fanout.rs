//! Cross-rate fan-out integration tests for rate-annotated array nodes.
//! Exercises the four shapes (Scalar / Broadcast / FanIn / Parallel) across
//! stream / value / event endpoints over a rate boundary.

use oscen::graph::{EventInput, EventInstance, EventPayload, StreamInput, StreamOutput, ValueInput, ValueOutput};
use oscen::{graph, Node, SignalProcessor};

/// Trivial value-passthrough node: copies its value input into its value
/// output every tick. Combined with a `LatchUp` cross-rate kernel, this
/// node lets a test observe whether the latched value reached every dest
/// element after an outer tick.
#[derive(Debug, Node)]
pub struct ValueLatch {
    pub input: ValueInput<f32>,
    pub output: ValueOutput<f32>,
}

impl ValueLatch {
    pub fn new() -> Self {
        Self {
            input: ValueInput::default(),
            output: ValueOutput(0.0),
        }
    }
}

impl Default for ValueLatch {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for ValueLatch {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = *self.input;
    }
}

graph! {
    name: BroadcastValueOversampled;
    input value src = 0.0;
    nodes {
        latches = [ValueLatch::new(); 4] * 2;
    }
    connections {
        src -> latches.input;
    }
}

#[test]
fn broadcast_value_outer_to_oversampled_array() {
    let mut g = BroadcastValueOversampled::new();
    g.init(48_000.0);
    g.src = 0.7;
    for _ in 0..8 {
        g.process();
    }
    // After K outer ticks the LatchUp kernel will have written 0.7 into the
    // input field on every inner tick of every element, so each element's
    // process() (running 2x per outer tick) will have copied 0.7 to its
    // output.
    for i in 0..4 {
        let got = *g.latches[i].output;
        assert!(
            (got - 0.7).abs() < 1e-6,
            "latches[{i}].output = {got}, expected 0.7"
        );
    }
}

/// Trivial DC-emitting node: outputs constant `value` (set at construction).
/// Used to verify cross-rate fan-in sums correctly across N elements.
#[derive(Debug, Node)]
pub struct DcEmitter {
    pub output: StreamOutput,
    value: f32,
}

impl DcEmitter {
    pub fn new() -> Self {
        Self {
            output: StreamOutput::default(),
            value: 1.0,
        }
    }
}

impl Default for DcEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for DcEmitter {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = self.value;
    }
}

graph! {
    name: FanInStreamArrayToScalar;
    output stream out;
    nodes {
        emitters = [DcEmitter::new(); 4] * 2;
    }
    connections {
        [sinc] emitters.output -> out;
    }
}

#[test]
fn fanin_stream_oversampled_array_to_outer_scalar() {
    let mut g = FanInStreamArrayToScalar::new();
    g.init(48_000.0);
    // Each emitter outputs 1.0; with 4 emitters fan-in sum = 4.0.
    // Run enough samples for the sinc downsampler to settle past its
    // group-delay transient.
    g.process_block(256);
    let written = &g.out_block[..256];
    // Look in the back half so the sinc kernel is past its warmup.
    let tail = &written[192..256];
    let avg: f32 = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(
        (avg - 4.0).abs() < 0.05,
        "expected fan-in sum ≈ 4.0 after sinc settles, got avg = {avg} over tail = {tail:?}"
    );
}

/// Holds a configurable f32 in `value`, and emits it on its `output` value
/// endpoint every tick. Used as the source of per-element distinct values.
#[derive(Debug, Node)]
pub struct ValueHolder {
    pub output: ValueOutput<f32>,
    pub value: f32,
}

impl ValueHolder {
    pub fn new() -> Self {
        Self {
            output: ValueOutput(0.0),
            value: 0.0,
        }
    }
}

impl Default for ValueHolder {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for ValueHolder {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = self.value;
    }
}

graph! {
    name: ParallelValueArrayToOversampledArray;
    nodes {
        sources = [ValueHolder::new(); 4];     // outer-rate
        latches = [ValueLatch::new(); 4] * 2;  // inner-rate, oversampled
    }
    connections {
        // Explicit `[latch]` forces the cross-rate Parallel resampler path:
        // without an explicit policy, the codegen's "both endpoint kinds
        // unknown" heuristic (rate_analysis::refine_with_types) collapses
        // node-to-node cross-rate edges to a same-rate ConnectEndpoints copy,
        // which would coincidentally pass this test for the wrong reason.
        [latch] sources.output -> latches.input;
    }
}

#[test]
fn parallel_value_array_to_oversampled_array_independent_states() {
    let mut g = ParallelValueArrayToOversampledArray::new();
    g.init(48_000.0);
    // Distinct values per element. If codegen accidentally classified this as
    // Broadcast (single shared resampler), every dest element would latch the
    // same value. Parallel must keep them independent.
    g.sources[0].value = 0.1;
    g.sources[1].value = 0.3;
    g.sources[2].value = 0.5;
    g.sources[3].value = 0.7;
    for _ in 0..8 {
        g.process();
    }
    let expected = [0.1_f32, 0.3, 0.5, 0.7];
    for (i, want) in expected.iter().enumerate() {
        let got = *g.latches[i].output;
        assert!(
            (got - want).abs() < 1e-6,
            "latches[{i}].output = {got}, expected {want}"
        );
    }
}

/// Captures the inner-rate `frame_offset` of the most recent gate event.
/// Used to assert event broadcast into an oversampled array preserves
/// the existing Multiply(N) frame-offset rescale.
#[derive(Debug, Node)]
pub struct EventOffsetCapture {
    pub gate: EventInput,
    pub captured_offset: ValueOutput<f32>,
}

impl EventOffsetCapture {
    pub fn new() -> Self {
        Self {
            gate: EventInput::default(),
            captured_offset: ValueOutput(-1.0),
        }
    }

    pub fn on_gate(&mut self, ev: &EventInstance) {
        *self.captured_offset = ev.frame_offset as f32;
    }
}

impl Default for EventOffsetCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for EventOffsetCapture {
    #[inline(always)]
    fn process(&mut self) {}
}

graph! {
    name: BroadcastEventOversampled;
    input event gate;
    nodes {
        captures = [EventOffsetCapture::new(); 4] * 2;
    }
    connections {
        gate -> captures.gate;
    }
}

#[test]
fn broadcast_event_outer_to_oversampled_array_with_rescale() {
    let mut g = BroadcastEventOversampled::new();
    g.init(48_000.0);
    let _ = g.gate.try_push(EventInstance {
        frame_offset: 1,
        payload: EventPayload::Scalar(1.0),
    });
    g.process_block(64);
    // With * 2 oversampling, frame_offset=1 should be rescaled to 1*2=2 on
    // the inner-rate side, captured by every element's gate handler.
    for i in 0..4 {
        let got = *g.captures[i].captured_offset;
        assert!(
            (got - 2.0).abs() < 1e-6,
            "captures[{i}].captured_offset = {got}, expected 2.0 (rescaled from outer offset 1)"
        );
    }
}

/// Mock voice with one of every endpoint kind. Models c15-synth's voice
/// surface area without bringing in the full DSP. The gate handler latches
/// `gate_seen = true`; subsequent process() ticks emit a constant so we can
/// detect that the voice activated.
#[derive(Debug, Node)]
pub struct MockVoice {
    pub freq: ValueInput<f32>,
    pub gate: EventInput,
    pub mod_in: StreamInput,
    pub audio_out: StreamOutput,
    gate_seen: bool,
}

impl MockVoice {
    pub fn new() -> Self {
        Self {
            freq: ValueInput::default(),
            gate: EventInput::default(),
            mod_in: StreamInput::default(),
            audio_out: StreamOutput::default(),
            gate_seen: false,
        }
    }

    pub fn on_gate(&mut self, _ev: &EventInstance) {
        self.gate_seen = true;
    }
}

impl Default for MockVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for MockVoice {
    #[inline(always)]
    fn process(&mut self) {
        // After a gate, emit a constant signal proportional to freq+mod so
        // the test can verify the voice actually ran. Before the gate, stays
        // silent — proves event delivery.
        *self.audio_out = if self.gate_seen {
            *self.freq * 0.001 + *self.mod_in
        } else {
            0.0
        };
    }
}

graph! {
    name: C15ShapeArrayAt2x;
    input value frequency = 440.0;
    input stream mod_signal;
    input event gate;
    output stream audio_out;

    nodes {
        voices = [MockVoice::new(); 8] * 2;
    }

    connections {
        frequency  -> voices.freq;
        mod_signal -> voices.mod_in;
        gate       -> voices.gate;
        [sinc] voices.audio_out -> audio_out;
    }
}

#[test]
fn c15_voice_array_at_2x_compiles_and_processes() {
    let mut g = C15ShapeArrayAt2x::new();
    g.init(48_000.0);
    g.frequency = 440.0;

    // Fill the mod_signal block with a small constant; the per-frame
    // process_block path reads from `<input>_block`.
    for s in g.mod_signal_block.iter_mut().take(256) {
        *s = 0.1;
    }

    let _ = g.gate.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    });
    g.process_block(256);

    let written = &g.audio_out_block[..256];
    // After the gate fires, every voice emits ~freq*0.001 + 0.1 = 0.54 per
    // tick. Sinc fan-in sum across 8 voices ≈ 8 * 0.54 = 4.32.
    let tail = &written[192..256];
    let avg: f32 = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(
        avg.abs() > 1.0,
        "expected non-zero audio after gate (avg = {avg} over tail = {tail:?})"
    );
}

graph! {
    name: RampedBroadcastToOversampled;
    // Ramped graph value input. Without the source-side `.current` special
    // case in `connection_source_value_expr`, this would emit a
    // `ConnectEndpoints<ValueRampState, f32>` call with no impl. Triggered
    // by c15-synth in real-world use.
    input value gain = 0.0 [0.0..1.0, ramp: 16];
    nodes {
        latches = [ValueLatch::new(); 4] * 2;
    }
    connections {
        gain -> latches.input;
    }
}

#[test]
fn ramped_value_input_broadcast_cross_rate() {
    let mut g = RampedBroadcastToOversampled::new();
    g.init(48_000.0);
    g.gain.set_immediate(0.5);
    // Ramp annotation is 16 frames; run enough outer ticks for it to settle
    // and the cross-rate latch to propagate.
    for _ in 0..32 {
        g.process();
    }
    for i in 0..4 {
        let got = *g.latches[i].output;
        assert!(
            (got - 0.5).abs() < 1e-3,
            "latches[{i}].output = {got}, expected ≈ 0.5 after ramp settles"
        );
    }
}
