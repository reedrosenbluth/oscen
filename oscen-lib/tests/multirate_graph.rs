use oscen::{graph, PolyBlepOscillator, SignalProcessor};

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
