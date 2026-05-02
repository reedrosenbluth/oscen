use oscen::graph::{StreamInput, StreamOutput};
use oscen::{graph, AdsrEnvelope, Node, PolyBlepOscillator, SignalProcessor};

/// Simple symmetric hard-clipper used by the aliasing-reduction test below.
/// Clipping is a memoryless nonlinearity that generates infinite harmonics; at
/// high input frequencies those harmonics fold back as aliasing when sampled at
/// the host rate. Running the same clipper at 4× internal rate moves the fold
/// point above the outer Nyquist for the first several harmonics, so the
/// downsampler's anti-alias filter can reject them.
#[derive(Debug, Node)]
pub struct HardClip {
    pub input: StreamInput,
    pub output: StreamOutput,
}

impl HardClip {
    pub fn new() -> Self {
        Self {
            input: StreamInput::default(),
            output: StreamOutput::default(),
        }
    }
}

impl Default for HardClip {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for HardClip {
    #[inline(always)]
    fn process(&mut self) {
        *self.output = (*self.input).clamp(-0.7, 0.7);
    }
}

graph! {
    name: MultiPass;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::saw(220.0, 0.6) * 4;
    }
    connections {
        [sinc] osc.output -> audio_out;
    }
}

graph! {
    name: PassRef;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::saw(220.0, 0.6);
    }
    connections {
        osc.output -> audio_out;
    }
}

#[test]
fn multirate_passthrough_low_freq_preserved() {
    let mut g = MultiPass::new();
    g.init(48_000.0);
    g.process_block(256);
    let written = &g.audio_out_block[..256];
    let max = written.iter().cloned().fold(0.0_f32, f32::max);
    let min = written.iter().cloned().fold(0.0_f32, f32::min);
    assert!(max > 0.5, "expected saw to swing positive (max = {max})");
    assert!(min < -0.3, "expected saw to swing negative (min = {min})");
}

#[test]
fn multirate_matches_reference_low_freq() {
    let mut a = MultiPass::new();
    let mut b = PassRef::new();
    a.init(48_000.0);
    b.init(48_000.0);

    // process_block is capped at MAX_BLOCK_SIZE (512). Run multiple chunks
    // and concatenate the per-chunk output buffers so we have enough samples
    // to absorb the sinc filter latency and still measure a meaningful MSE.
    const CHUNK: usize = 256;
    const TOTAL: usize = 2048;
    assert!(TOTAL % CHUNK == 0);

    let mut xs_full = Vec::with_capacity(TOTAL);
    let mut ys_full = Vec::with_capacity(TOTAL);
    for _ in 0..(TOTAL / CHUNK) {
        a.process_block(CHUNK);
        b.process_block(CHUNK);
        xs_full.extend_from_slice(&a.audio_out_block[..CHUNK]);
        ys_full.extend_from_slice(&b.audio_out_block[..CHUNK]);
    }

    let warmup = 64;
    let xs = &xs_full[warmup..TOTAL];
    let ys = &ys_full[warmup..TOTAL];
    // Sinc adds latency we don't account for here. Try a range of lags and
    // pick the one with smallest MSE.
    let mut best_mse = f32::INFINITY;
    for lag in 0..32 {
        if lag >= xs.len() { break; }
        let n = xs.len().saturating_sub(lag).min(ys.len());
        if n == 0 { continue; }
        let mse: f32 = (0..n).map(|i| {
            let d = xs[i] - ys[i + lag];
            d * d
        }).sum::<f32>() / n as f32;
        if mse < best_mse { best_mse = mse; }
    }
    assert!(best_mse < 0.05, "MSE between 4×-resampled and reference = {best_mse}");
}

#[test]
fn multirate_reports_nonzero_latency() {
    let g = MultiPass::new();
    assert!(g.latency_samples() > 0, "4×→1× sinc downsampler should report > 0 latency");
}

#[test]
fn samerate_reports_zero_latency() {
    let g = PassRef::new();
    assert_eq!(g.latency_samples(), 0);
}

// ---------------------------------------------------------------------------
// Event routing across rate boundaries (Phase 5 Task 5.1)
//
// v1 limitation: `EventInstance::frame_offset` is not rescaled across cross-rate
// edges (see Known Limitations in
// docs/superpowers/specs/2026-05-01-multirate-graph-design.md). However events
// scheduled at frame_offset == 0 — the dominant case after `process_block`'s
// sub-block split aligns events to outer-tick boundaries — must still be
// delivered to inner-rate (`* N`) nodes. This smoke test guards basic event
// delivery across the rate boundary; it does NOT assert exact frame-accurate
// inner-tick semantics.

graph! {
    name: EventOversampledGraph;
    input event gate;
    output stream audio_out;
    nodes {
        env = AdsrEnvelope::new(0.005, 0.05, 0.7, 0.05) * 4;
    }
    connections {
        gate -> env.gate;
        env.output -> audio_out;
    }
}

#[test]
fn event_routed_to_oversampled_node_at_offset_zero() {
    use oscen::graph::{EventInstance, EventPayload};

    let mut g = EventOversampledGraph::new();
    g.init(48_000.0);

    // Push a gate-on event at frame_offset = 0. After process_block runs the
    // sub-block split, this should drive the (* 4) ADSR's gate handler on the
    // first outer-tick boundary, opening the envelope.
    let _ = g.gate.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    });

    // Long enough for the 5 ms attack at 48 kHz to clearly rise above zero.
    g.process_block(256);

    let written = &g.audio_out_block[..256];
    let max = written.iter().cloned().fold(0.0_f32, f32::max);
    assert!(
        max > 0.1,
        "expected ADSR to open after gate-on event reaches the *4 node \
         (max envelope output = {max})"
    );
}

// ---------------------------------------------------------------------------
// Value latch across rate boundaries (Phase 5 Task 5.2)
//
// Values are piecewise-constant. When a graph-level value input feeds an
// inner-rate (`* N`) node's value-input field via the `[latch]` policy, the
// `LatchUp` kernel writes the same value into all N inner-tick slots of the
// per-edge buffer; the inner node's field is then assigned that value on each
// inner tick. Because the field is not cleared between inner ticks, every
// inner `process()` observes the same constant — exactly the latch semantic
// the design spec calls for. No special codegen path is required.

graph! {
    name: ValueLatchOversampledGraph;
    input value amp = 1.0;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::saw(220.0, 1.0) * 4;
    }
    connections {
        [latch] amp -> osc.amplitude;
        [sinc] osc.output -> audio_out;
    }
}

#[test]
fn value_latched_into_oversampled_node() {
    let mut g = ValueLatchOversampledGraph::new();
    g.init(48_000.0);

    // Set the graph-level amplitude value to a known constant. The latch
    // upsampler should propagate this verbatim into the *4 oscillator's
    // amplitude field every inner tick.
    g.set_amp(0.25);

    // Process enough samples for the saw to swing through a full period and
    // for the sinc downsampler at the output to reach steady state.
    g.process_block(512);

    let written = &g.audio_out_block[..512];
    let max = written.iter().cloned().fold(0.0_f32, f32::max);
    let min = written.iter().cloned().fold(0.0_f32, f32::min);

    // A 220 Hz saw at amplitude=0.25 should swing within ±0.25 (plus a small
    // margin for sinc downsampler ringing and PolyBLEP overshoot at the
    // discontinuity). If the value were not latched (e.g., dropped to zero
    // between inner ticks) the output would collapse toward 0.
    assert!(
        max > 0.15,
        "expected scaled saw to swing positive ~0.25 (max = {max})"
    );
    assert!(
        min < -0.05,
        "expected scaled saw to swing negative (min = {min})"
    );
    // Loose upper bound: amplitude=0.25 should keep the magnitude well under
    // amplitude=1.0's typical swing. Sinc filtering can overshoot the saw
    // discontinuity slightly so allow some headroom above 0.25.
    let peak = max.max(-min);
    assert!(
        peak < 0.5,
        "amplitude=0.25 should keep peak well below 1.0 (peak = {peak})"
    );
}

// ---------------------------------------------------------------------------
// Aliasing reduction across rate boundary (Phase 6 Task 6.1)
//
// Hard-clipping a sine generates an infinite harmonic series. At 1× sample
// rate any harmonic above outer Nyquist folds back into audible range and
// shows up as inharmonic aliasing energy. Running the same clipper at 4×
// internal rate raises the alias-fold threshold to 4× outer Nyquist, so the
// downsampler's anti-alias filter can reject the first several harmonics
// before they fold. The 4× graph should therefore exhibit measurably less
// aliasing energy in the band below outer Nyquist.

graph! {
    name: ClipRef;
    input stream input;
    output stream out;
    nodes {
        clip = HardClip::new();
    }
    connections {
        input -> clip.input;
        clip.output -> out;
    }
}

graph! {
    name: ClipOversampled;
    input stream input;
    output stream out;
    nodes {
        clip = HardClip::new() * 4;
    }
    connections {
        [sinc] input -> clip.input;
        [sinc] clip.output -> out;
    }
}

#[test]
fn hardclip_4x_has_less_aliasing_than_1x() {
    let mut a = ClipRef::new();
    let mut b = ClipOversampled::new();
    a.init(48_000.0);
    b.init(48_000.0);

    // Drive both with a high-frequency sine that will alias when clipped at 1×.
    // f = 9_600 Hz at sr=48k → normalized to 0.2 cycles/sample at outer rate.
    // The 3rd harmonic (3*f) lands at 0.6 cycles/sample, which folds back to
    // 0.4. The 5th (1.0) folds to 0.0. We measure aliasing energy at 0.4
    // (the 3rd-harmonic alias) relative to the fundamental at 0.2.
    let f = 9_600.0_f32 / 48_000.0;
    let block = 256;
    let total = 4096;
    let warmup = 512;

    let mut a_out = Vec::with_capacity(total);
    let mut b_out = Vec::with_capacity(total);

    let mut sample_n: usize = 0;
    while sample_n < total {
        let chunk = block.min(total - sample_n);
        for i in 0..chunk {
            let n = sample_n + i;
            let amp = 0.9_f32; // exceed clipping threshold (±0.7)
            let x = amp * (2.0 * std::f32::consts::PI * f * n as f32).sin();
            a.input_block[i] = x;
            b.input_block[i] = x;
        }
        a.process_block(chunk);
        b.process_block(chunk);
        a_out.extend_from_slice(&a.out_block[..chunk]);
        b_out.extend_from_slice(&b.out_block[..chunk]);
        sample_n += chunk;
    }

    let f_fundamental = 0.2_f32;
    let f_alias = 0.4_f32;
    let span = total - warmup;
    let one_x_alias = bin_magnitude(&a_out[warmup..], f_alias, span);
    let four_x_alias = bin_magnitude(&b_out[warmup..], f_alias, span);
    let one_x_fund = bin_magnitude(&a_out[warmup..], f_fundamental, span);
    let four_x_fund = bin_magnitude(&b_out[warmup..], f_fundamental, span);

    let one_x_ratio = one_x_alias / one_x_fund.max(1e-9);
    let four_x_ratio = four_x_alias / four_x_fund.max(1e-9);

    println!("1x aliasing/fundamental: {one_x_ratio}");
    println!("4x aliasing/fundamental: {four_x_ratio}");

    assert!(
        four_x_ratio < one_x_ratio * 0.5,
        "4× should have <50% the aliasing ratio of 1×: 4x={four_x_ratio}, 1x={one_x_ratio}"
    );
}

// ---------------------------------------------------------------------------
// Mixed-rate graph: two oversampled branches summed at outer rate (Phase 6 Task 6.2)
//
// Verifies that two independent 4× oversampled branches can be combined at the
// outer (host) rate via a binary expression connection. Exercises multi-rate
// codegen with multiple cross-rate edges feeding a same-rate downstream sum.

graph! {
    name: TwoStage;
    output stream out;
    output stream out_a;
    output stream out_b;
    nodes {
        a = PolyBlepOscillator::saw(220.0, 0.5) * 4;
        b = PolyBlepOscillator::saw(330.0, 0.5) * 4;
    }
    connections {
        [sinc] a.output -> out_a;
        [sinc] b.output -> out_b;
        out_a + out_b -> out;
    }
}

#[test]
fn two_oversampled_branches_sum_to_outer() {
    let mut g = TwoStage::new();
    g.init(48_000.0);
    let block = 256;
    let total = 1024;
    let mut sum_block = Vec::with_capacity(total);
    let mut n = 0;
    while n < total {
        let chunk = block.min(total - n);
        g.process_block(chunk);
        sum_block.extend_from_slice(&g.out_block[..chunk]);
        n += chunk;
    }
    let warmup = 100;
    let mixed = &sum_block[warmup..];
    let max = mixed.iter().cloned().fold(0.0_f32, f32::max);
    let min = mixed.iter().cloned().fold(0.0_f32, f32::min);
    assert!(max > 0.7, "two saws summed should swing > 0.7 (max = {max})");
    assert!(min < -0.5, "two saws summed should swing < -0.5 (min = {min})");
}

/// Single-bin DFT magnitude at `freq` (cycles/sample), normalized by N.
fn bin_magnitude(samples: &[f32], freq: f32, n: usize) -> f32 {
    let mut re = 0.0_f32;
    let mut im = 0.0_f32;
    let omega = 2.0 * std::f32::consts::PI * freq;
    for (i, &x) in samples.iter().take(n).enumerate() {
        let phase = omega * i as f32;
        re += x * phase.cos();
        im += x * phase.sin();
    }
    (re * re + im * im).sqrt() / n as f32
}
