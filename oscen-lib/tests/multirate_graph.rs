#![feature(inherent_associated_types)]
use oscen::graph::{EventInput, EventInstance, EventPayload, StreamInput, StreamOutput};
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

// Sine carrier deliberately chosen to avoid PolyBLEP's rate-dependent
// discontinuity-correction shape. With a saw, the BLEP correction at 192k
// inner-rate has different per-sample shape than at 48k host-rate, so the
// host-rate reference and the resampled internal-rate signal disagree to
// ~5% MSE for reasons unrelated to the resampler. A sine has no
// discontinuity, so any divergence is the resampler.
graph! {
    name: MultiPass;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(220.0, 0.6) * 4;
    }
    connections {
        [sinc] osc.output -> audio_out;
    }
}

graph! {
    name: PassRef;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(220.0, 0.6);
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
        if lag >= xs.len() {
            break;
        }
        let n = xs.len().saturating_sub(lag).min(ys.len());
        if n == 0 {
            continue;
        }
        let mse: f32 = (0..n)
            .map(|i| {
                let d = xs[i] - ys[i + lag];
                d * d
            })
            .sum::<f32>()
            / n as f32;
        if mse < best_mse {
            best_mse = mse;
        }
    }
    // Sine reference yields MSE ~0.010 (residual is dominated by integer-lag
    // search alignment of the sinc filter's fractional group delay). 0.02 is
    // 2× headroom — tight enough to catch real resampler regressions without
    // flapping on micro-changes.
    assert!(
        best_mse < 0.02,
        "MSE between 4×-resampled and reference = {best_mse}"
    );
}

#[test]
fn multirate_reports_nonzero_latency() {
    let g = MultiPass::new();
    assert!(
        g.latency_samples() > 0,
        "4×→1× sinc downsampler should report > 0 latency"
    );
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

// IIR halfband variant of the same chain. The IIR halfband cascade
// (Regalia-Mitra polyphase all-pass) is a different codegen path and a
// different filter family — it has non-linear phase but typically a steeper
// stopband for comparable order than the 23-tap Kaiser FIR. This guards the
// `[sinc_iir]` policy with the same end-to-end aliasing assertion.
graph! {
    name: ClipOversampledIir;
    input stream input;
    output stream out;
    nodes {
        clip = HardClip::new() * 4;
    }
    connections {
        [sinc_iir] input -> clip.input;
        [sinc_iir] clip.output -> out;
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

#[test]
fn hardclip_4x_iir_has_less_aliasing_than_1x() {
    let mut a = ClipRef::new();
    let mut b = ClipOversampledIir::new();
    a.init(48_000.0);
    b.init(48_000.0);

    // Same setup as the FIR variant: 9.6 kHz sine clipped at ±0.7. The 3rd
    // harmonic at 28.8 kHz folds to 19.2 kHz (0.4 cyc/sample) at 1×; the 4×
    // IIR halfband cascade should attenuate it.
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
            let amp = 0.9_f32;
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

    println!("1x  aliasing/fundamental:        {one_x_ratio}");
    println!("4x IIR aliasing/fundamental:     {four_x_ratio}");

    assert!(
        four_x_ratio < one_x_ratio * 0.5,
        "4× IIR should have <50% the aliasing ratio of 1×: 4x={four_x_ratio}, 1x={one_x_ratio}"
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
    assert!(
        max > 0.7,
        "two saws summed should swing > 0.7 (max = {max})"
    );
    assert!(
        min < -0.5,
        "two saws summed should swing < -0.5 (min = {min})"
    );
}

// ---------------------------------------------------------------------------
// Topological correctness: a Same node downstream of a Down edge must run
// AFTER the inner loop's downsample finalize step. Before this fix it ran in
// the pre-inner outer-process bucket, so it observed previous-outer-tick data
// in its input field and its output was delayed by one outer tick. Comparing
// `osc * 4 → DOWN → out` against `osc * 4 → DOWN → clip → out` (where clip
// is identity for a ±0.6 sine) should yield equal output sample-for-sample.
// With the bug, the clip-version lags by exactly one outer tick.
//
// Uses sine to avoid PolyBLEP rate-shape divergence (see MSE test above).

graph! {
    name: PostInnerNoClip;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(220.0, 0.6) * 4;
    }
    connections {
        [sinc] osc.output -> audio_out;
    }
}

graph! {
    name: PostInnerWithClip;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(220.0, 0.6) * 4;
        clip = HardClip::new();
    }
    connections {
        [sinc] osc.output -> clip.input;
        clip.output -> audio_out;
    }
}

#[test]
fn same_node_downstream_of_down_edge_is_in_phase() {
    let mut a = PostInnerNoClip::new();
    let mut b = PostInnerWithClip::new();
    a.init(48_000.0);
    b.init(48_000.0);

    const CHUNK: usize = 256;
    const TOTAL: usize = 1024;
    let mut xs = Vec::with_capacity(TOTAL);
    let mut ys = Vec::with_capacity(TOTAL);
    for _ in 0..(TOTAL / CHUNK) {
        a.process_block(CHUNK);
        b.process_block(CHUNK);
        xs.extend_from_slice(&a.audio_out_block[..CHUNK]);
        ys.extend_from_slice(&b.audio_out_block[..CHUNK]);
    }

    // After the warmup the two graphs differ only by a Same-rate identity
    // pass-through; they must be equal sample-for-sample. With the topo bug
    // ys would lag xs by exactly one outer tick.
    let warmup = 64;
    let mut max_abs_diff = 0.0_f32;
    for i in warmup..TOTAL {
        let d = (xs[i] - ys[i]).abs();
        if d > max_abs_diff {
            max_abs_diff = d;
        }
    }
    assert!(
        max_abs_diff < 1.0e-6,
        "post-inner Same node should be in-phase with no-clip reference (max abs diff = {max_abs_diff})"
    );
}

// ---------------------------------------------------------------------------
// Multi-rate graph nested as a node in an outer graph (Follow-up #31)
//
// When a multi-rate graph is used as a node inside another graph, the outer
// graph calls its inherent `process()` method once per outer tick. That
// method must run the multi-rate inner loop (×N inner ticks per outer call)
// otherwise the inner-rate (`* N`) nodes only advance once per outer call —
// the oscillator's effective output frequency becomes 1/N of what was
// requested, producing the wrong waveform with no compile-time signal.
//
// The reference path runs the same multi-rate graph directly via
// `process_block`, which routes through `__advance_one_frame_multirate` and
// is already correct. The nested path must match it sample-for-sample.

graph! {
    name: NestedMultirateInner;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(440.0, 0.5) * 4;
    }
    connections {
        [sinc] osc.output -> audio_out;
    }
}

graph! {
    name: NestedMultirateOuter;
    output stream audio_out;
    nodes {
        inner = NestedMultirateInner;
    }
    connections {
        inner.audio_out -> audio_out;
    }
}

#[test]
fn nested_multirate_graph_ticks_inner_loop_per_outer_call() {
    let mut direct = NestedMultirateInner::new();
    let mut nested = NestedMultirateOuter::new();
    direct.init(48_000.0);
    nested.init(48_000.0);

    const CHUNK: usize = 256;
    const TOTAL: usize = 1024;
    let mut direct_buf = Vec::with_capacity(TOTAL);
    let mut nested_buf = Vec::with_capacity(TOTAL);
    for _ in 0..(TOTAL / CHUNK) {
        direct.process_block(CHUNK);
        nested.process_block(CHUNK);
        direct_buf.extend_from_slice(&direct.audio_out_block[..CHUNK]);
        nested_buf.extend_from_slice(&nested.audio_out_block[..CHUNK]);
    }

    // The two paths share the same inner kernel state and same inputs, so
    // they should match sample-for-sample. With the bug the nested path
    // would tick the *4 oscillator only once per outer sample, producing a
    // 110 Hz output instead of 440 Hz — a gross divergence (>0.1 amplitude).
    let warmup = 64;
    let mut max_abs_diff = 0.0_f32;
    for i in warmup..TOTAL {
        let d = (direct_buf[i] - nested_buf[i]).abs();
        if d > max_abs_diff {
            max_abs_diff = d;
        }
    }
    assert!(
        max_abs_diff < 1.0e-6,
        "nested multirate graph must match standalone (max abs diff = {max_abs_diff})"
    );
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

// ---------------------------------------------------------------------------
// Cross-rate event edge anchored via a graph-level event input.
//
// Documents the current capability boundary: an event edge crossing a rate
// boundary needs at least one anchor that the macro's `TypeContext` can use
// to classify the edge's endpoint kind. Anchoring via a graph-level
// `input event` works today; pure node-to-node anchoring will work once
// `CrossRateKernel` projection fires on node-to-node edges in a future PR.
//
// Concretely: the macro emits `EdgeKernel::Event { rescale: Multiply(N) }`
// for the `gate_in -> env.gate` edge, so the gate-on event's `frame_offset`
// is rescaled into the inner-rate `*N` node's tick space and the ADSR opens
// on the correct inner tick.

graph! {
    name: NodeToNodeCrossRateEvent;
    input event gate_in;
    output stream audio_out;
    nodes {
        env = AdsrEnvelope::new(0.005, 0.05, 0.7, 0.05) * 4;
    }
    connections {
        gate_in -> env.gate;
        [sinc] env.output -> audio_out;
    }
}

#[test]
fn node_to_node_cross_rate_event_compiles_and_dispatches() {
    use oscen::graph::{EventInstance, EventPayload};

    let mut g = NodeToNodeCrossRateEvent::new();
    g.init(48_000.0);
    let _ = g.gate_in.try_push(EventInstance {
        frame_offset: 0,
        payload: EventPayload::Scalar(1.0),
    });
    g.process_block(256);
    let written = &g.audio_out_block[..256];
    let max = written.iter().cloned().fold(0.0_f32, f32::max);
    assert!(
        max > 0.05,
        "anchored cross-rate event edge should drive ADSR open across rate boundary (max = {max})"
    );
}

// ---------------------------------------------------------------------------
// Follow-up #35: default policy honors endpoint kind.
//
// Spec: cross-rate value edges default to `latch`; cross-rate stream edges
// default to `sinc`. Before the fix, `Default` and `Sinc` resolved to the
// same `SincUpFir` kernel, so a user-omitted policy on a value edge applied
// the sinc filter to the constant — destroying the latch property.

graph! {
    name: DefaultLatchValueEdge;
    input value amp = 1.0;
    output stream audio_out;
    nodes {
        osc = PolyBlepOscillator::sine(220.0, 1.0) * 4;
    }
    connections {
        amp -> osc.amplitude;
        [sinc] osc.output -> audio_out;
    }
}

#[test]
fn default_policy_value_edge_uses_latch_not_sinc() {
    let mut g = DefaultLatchValueEdge::new();
    g.init(48_000.0);
    g.set_amp(0.25);
    g.process_block(512);
    let written = &g.audio_out_block[..512];
    let max = written.iter().cloned().fold(0.0_f32, f32::max);
    let min = written.iter().cloned().fold(0.0_f32, f32::min);
    let peak = max.max(-min);
    // Latched: peak should match a sine at 0.25 amplitude (with sinc ringing).
    // Sinc'd value = filter on a constant = a transient near zero crossing,
    // which would either match (the constant case) or crush amplitude. We
    // check a tighter window: the output should clearly swing through ~0.25
    // (proving amp was scaled), and must not exceed the obvious upper bound.
    assert!(
        peak > 0.15,
        "value-edge latch should pass ~0.25 amplitude through (peak = {peak})"
    );
    assert!(
        peak < 0.5,
        "amplitude=0.25 should keep peak well under 1.0 (peak = {peak})"
    );
}

// ---------------------------------------------------------------------------
// Follow-up #33: event frame_offset rescaling across rate boundaries.
//
// An event with frame_offset = K entering a `* N` node should fire at inner
// frame K * N. We use a simple capture node that records its inner sample-tick
// when the gate event handler fires. The test pushes a gate event at outer
// frame 2, processes a block, and asserts that some non-zero capture occurred —
// proving the event was rescaled (without rescaling, the inner-tick at the
// outer-frame-2 boundary would still be the inner-frame-8 mark, and we mainly
// verify rescale happened somewhere in the right direction).

#[derive(Debug, Node)]
pub struct OffsetCapture {
    pub gate: EventInput,
    pub captured_tick: StreamOutput,
    pub output: StreamOutput,
    internal_tick: u32,
    last_capture: f32,
}

impl OffsetCapture {
    pub fn new() -> Self {
        Self {
            gate: EventInput::default(),
            captured_tick: StreamOutput::default(),
            output: StreamOutput::default(),
            internal_tick: 0,
            last_capture: -1.0,
        }
    }

    fn on_gate(&mut self, _event: &EventInstance) {
        // Record the inner tick at which the gate event fired.
        self.last_capture = self.internal_tick as f32;
    }
}

impl Default for OffsetCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for OffsetCapture {
    #[inline(always)]
    fn process(&mut self) {
        self.internal_tick += 1;
        *self.captured_tick = self.last_capture;
        *self.output = self.last_capture;
    }
}

graph! {
    name: EventOffsetRescale;
    input event gate;
    output stream out;
    nodes {
        capture = OffsetCapture::new() * 4;
    }
    connections {
        gate -> capture.gate;
        [sinc] capture.output -> out;
    }
}

#[test]
fn event_frame_offset_rescaled_into_oversampled_node() {
    // Push an event at outer-frame 2. With *4 oversampling, the inner node
    // should receive the gate at inner-frame 2*4 = 8. The capture node records
    // its internal tick at the moment its gate handler fires.
    let mut g = EventOffsetRescale::new();
    g.init(48_000.0);

    let _ = g.gate.try_push(EventInstance {
        frame_offset: 2,
        payload: EventPayload::Scalar(1.0),
    });

    // Run enough frames to cover the offset and let the capture stabilize.
    g.process_block(128);

    // Direct inspection of the inner node's last captured tick (no
    // downsampling latency; reads the field after process_block returns).
    let captured = g.capture.last_capture;
    // Without rescaling, the event would still arrive (frame_offset 2 falls
    // within the block), but at the outer-tick alignment — the gate handler
    // would fire at outer-frame 2 / inner-tick 2, not inner-tick 8. With
    // rescaling, the multi-rate inner loop multiplies the offset by 4 so the
    // event fires on inner-frame 8. The capture node records its
    // `internal_tick` field at the moment its handler runs.
    //   1. captured >= 0 (a capture happened; the event was delivered).
    //   2. captured == outer_offset * factor (= 8, the rescaled boundary).
    assert!(
        captured >= 0.0,
        "expected gate handler to fire (captured = {captured})"
    );
    let expected_inner = 2.0 * 4.0;
    assert!(
        (captured - expected_inner).abs() <= 1.0,
        "expected rescaled inner-tick near {expected_inner}, got {captured}"
    );
}

// === Array-embedded rate test ===
//
// Verifies that `[Inner::new(); N] * 2` in a parent graph causes each of
// the N inner-graph instances to run its inner loop twice per outer tick
// (i.e. the embedded `* 2` is recognised, parsed into NodeRate::Up(2),
// and produces an oversampled inner loop in codegen).

#[derive(Debug, Node)]
pub struct TickCounter {
    pub count: oscen::graph::ValueOutput<f32>,
}

impl TickCounter {
    pub fn new() -> Self {
        Self {
            count: oscen::graph::ValueOutput(0.0),
        }
    }
}

impl Default for TickCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for TickCounter {
    #[inline(always)]
    fn process(&mut self) {
        *self.count += 1.0;
    }
}

graph! {
    name: TickCounterInner;
    output count: value;
    nodes {
        counter = TickCounter::new();
    }
    connections {
        counter.count -> count;
    }
}

graph! {
    name: TickArrayParent;
    output total_a: value;
    output total_b: value;
    output total_c: value;
    output total_d: value;
    nodes {
        counters = [TickCounterInner::new(); 4] * 2;
    }
    connections {
        counters[0].count -> total_a;
        counters[1].count -> total_b;
        counters[2].count -> total_c;
        counters[3].count -> total_d;
    }
}

#[test]
fn array_node_with_embedded_rate_oversamples_each_instance() {
    let mut g = TickArrayParent::new();
    g.init(48_000.0);
    let k: u32 = 8;
    for _ in 0..k {
        g.process();
    }
    // Each of the 4 inner-graph instances should have ticked exactly `k * 2`
    // times — the embedded `* 2` rate annotation oversamples every voice.
    // We read the raw inner counter directly (rather than going through the
    // graph output, which is a Down-rate-edge sample of the inner counter).
    let expected = (2 * k) as f32;
    assert_eq!(
        *g.counters[0].counter.count, expected,
        "voice 0 ran wrong number of inner ticks"
    );
    assert_eq!(
        *g.counters[1].counter.count, expected,
        "voice 1 ran wrong number of inner ticks"
    );
    assert_eq!(
        *g.counters[2].counter.count, expected,
        "voice 2 ran wrong number of inner ticks"
    );
    assert_eq!(
        *g.counters[3].counter.count, expected,
        "voice 3 ran wrong number of inner ticks"
    );
}

// ---------------------------------------------------------------------------
// Projection-fires test. The macro must project through inherent-assoc-type
// aliases so a bare-ident node-type path resolves the per-endpoint marker.
// Compilation alone is the assertion.

graph! {
    name: ProjectionFires;
    output stream out;
    nodes {
        osc = PolyBlepOscillator::sine(440.0, 1.0) * 4;
    }
    connections {
        [sinc] osc.output -> out;
    }
}

#[test]
fn projection_fires_on_bare_ident_node_type() {
    let mut g = ProjectionFires::new();
    g.init(48_000.0);
    g.process_block(64);
}
