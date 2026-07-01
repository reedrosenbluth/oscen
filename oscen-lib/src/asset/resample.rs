//! Offline band-limited resampling for conforming loaded assets to the graph
//! rate.
//!
//! This is **not** the real-time `crate::resample` path (fixed-ratio halfband
//! polyphase running in the graph). It is a one-shot, arbitrary-ratio
//! windowed-sinc resample run off the audio thread inside the asset load path,
//! once per load — so it is free to allocate and to use a long, high-quality
//! kernel.

use std::f32::consts::PI;

/// Sinc zero-crossings on each side of the kernel center. A longer kernel is a
/// sharper transition band; 32 is a good offline-quality/cost tradeoff.
const ZERO_CROSSINGS: usize = 32;

/// Normalized sinc, `sin(pi x) / (pi x)`, with the removable singularity at
/// `x == 0` filled in.
fn sinc(x: f32) -> f32 {
    if x == 0.0 {
        1.0
    } else {
        let pix = PI * x;
        pix.sin() / pix
    }
}

/// Blackman window over `t in [-1, 1]` (zero outside). Used to taper the sinc
/// kernel to a finite support.
fn blackman(t: f32) -> f32 {
    if t.abs() > 1.0 {
        return 0.0;
    }
    // Standard Blackman with the argument mapped so t = 0 is the center.
    let phase = PI * (t + 1.0);
    0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()
}

/// Offline band-limited rational resample of a single channel from `src_rate`
/// to `dst_rate`. Both rates must be `> 0`.
///
/// Not real-time: allocates, `O(out_len * kernel_len)`; called once per asset
/// load, off the audio thread. A DC input maps to a DC output at unity gain,
/// and downsampling band-limits to the destination Nyquist (no aliasing).
pub(crate) fn resample_channel(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    debug_assert!(
        src_rate > 0 && dst_rate > 0,
        "sample rates must be non-zero"
    );
    if input.is_empty() || src_rate == dst_rate {
        return input.to_vec();
    }

    let ratio = dst_rate as f64 / src_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    if out_len == 0 {
        return Vec::new();
    }

    // Cutoff relative to the input Nyquist: pass the full input band when
    // upsampling; band-limit to the (lower) destination Nyquist when
    // downsampling, which is what prevents aliasing.
    let cutoff = ratio.min(1.0) as f32;
    // Kernel half-width in *input* samples. Widens as the cutoff drops so the
    // number of sinc zero-crossings under the window stays constant.
    let radius = ZERO_CROSSINGS as f32 / cutoff;

    let inv_ratio = 1.0 / ratio; // output index -> input position
    let mut output = Vec::with_capacity(out_len);
    for n in 0..out_len {
        // Continuous source position (in input samples) for this output sample.
        let pos = n as f64 * inv_ratio;
        let first = (pos - radius as f64).ceil() as isize;
        let last = (pos + radius as f64).floor() as isize;

        let mut acc = 0.0f32;
        let mut weight_sum = 0.0f32;
        for i in first..=last {
            if i < 0 || i as usize >= input.len() {
                continue;
            }
            // `dist` is in input samples from the kernel center. No leading
            // `cutoff` gain factor: a constant scale cancels under the
            // per-output normalization below.
            let dist = (pos - i as f64) as f32;
            let w = sinc(cutoff * dist) * blackman(dist / radius);
            acc += w * input[i as usize];
            weight_sum += w;
        }

        // Normalize by the weights actually applied: exact unity DC gain, and
        // clean handling of the truncated kernel at the buffer ends.
        output.push(if weight_sum != 0.0 {
            acc / weight_sum
        } else {
            0.0
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    /// A constant input must resample to the same constant (unity DC gain),
    /// both up and down.
    #[test]
    fn constant_is_preserved() {
        let input = vec![0.7f32; 500];
        for (src, dst) in [(48_000, 44_100), (44_100, 48_000), (96_000, 44_100)] {
            let out = resample_channel(&input, src, dst);
            // Skip the first/last few samples where the kernel runs off the end.
            let guard = ZERO_CROSSINGS + 4;
            for (i, &y) in out.iter().enumerate() {
                if i < guard || i + guard >= out.len() {
                    continue;
                }
                assert!(
                    approx_eq!(f32, y, 0.7, epsilon = 1e-3),
                    "constant not preserved at {src}->{dst}, idx {i}: {y}"
                );
            }
        }
    }

    /// A sub-Nyquist sine keeps its frequency (in Hz) and amplitude across a
    /// resample.
    #[test]
    fn sine_frequency_and_amplitude_preserved() {
        let src = 48_000u32;
        let dst = 44_100u32;
        let freq = 1_000.0f32; // well below both Nyquist limits
        let len = 24_000usize; // 0.5 s
        let input: Vec<f32> = (0..len)
            .map(|i| (2.0 * PI * freq * i as f32 / src as f32).sin())
            .collect();

        let out = resample_channel(&input, src, dst);

        // Compare against an ideal sine at the destination rate, away from the
        // edges where the kernel is truncated.
        let guard = ZERO_CROSSINGS + 8;
        let mut max_err = 0.0f32;
        for i in guard..out.len() - guard {
            let want = (2.0 * PI * freq * i as f32 / dst as f32).sin();
            max_err = max_err.max((out[i] - want).abs());
        }
        assert!(
            max_err < 1e-2,
            "sine mismatch after resample: max_err {max_err}"
        );
    }

    /// Downsampling band-limits: a tone above the destination Nyquist is
    /// strongly attenuated rather than aliased back into the baseband.
    #[test]
    fn downsample_rejects_above_nyquist() {
        let src = 48_000u32;
        let dst = 16_000u32; // destination Nyquist 8 kHz
        let freq = 12_000.0f32; // above 8 kHz -> must be rejected
        let len = 24_000usize;
        let input: Vec<f32> = (0..len)
            .map(|i| (2.0 * PI * freq * i as f32 / src as f32).sin())
            .collect();

        let out = resample_channel(&input, src, dst);

        let guard = ZERO_CROSSINGS + 8;
        let mut peak = 0.0f32;
        for &y in &out[guard..out.len() - guard] {
            peak = peak.max(y.abs());
        }
        assert!(peak < 0.1, "above-Nyquist tone not rejected: peak {peak}");
    }

    /// A 2:1 downsample of a smooth low-frequency signal reproduces every other
    /// input sample within a small tolerance (the band-limiting barely touches
    /// content this far below Nyquist).
    #[test]
    fn integer_downsample_matches_reference() {
        let src = 48_000u32;
        let dst = 24_000u32;
        let freq = 200.0f32;
        let len = 12_000usize;
        let input: Vec<f32> = (0..len)
            .map(|i| (2.0 * PI * freq * i as f32 / src as f32).sin())
            .collect();

        let out = resample_channel(&input, src, dst);

        let guard = ZERO_CROSSINGS + 8;
        for i in guard..out.len() - guard {
            let want = input[i * 2]; // 2:1 decimation reference
            assert!(
                (out[i] - want).abs() < 5e-3,
                "2:1 downsample mismatch at {i}: got {}, want {want}",
                out[i]
            );
        }
    }

    /// Output length tracks the resample ratio.
    #[test]
    fn output_length_tracks_ratio() {
        let input = vec![0.0f32; 1_000];
        assert_eq!(resample_channel(&input, 48_000, 24_000).len(), 500);
        assert_eq!(resample_channel(&input, 24_000, 48_000).len(), 2_000);
        assert_eq!(resample_channel(&input, 48_000, 48_000).len(), 1_000);
    }

    /// Upsampling reproduces a sub-Nyquist sine at the destination rate (the
    /// counterpart to `sine_frequency_and_amplitude_preserved`, which goes
    /// down): frequency in Hz and amplitude are preserved with no imaging.
    #[test]
    fn upsample_preserves_sine() {
        let src = 24_000u32;
        let dst = 48_000u32;
        let freq = 1_000.0f32; // well below both Nyquist limits
        let len = 12_000usize; // 0.5 s
        let input: Vec<f32> = (0..len)
            .map(|i| (2.0 * PI * freq * i as f32 / src as f32).sin())
            .collect();

        let out = resample_channel(&input, src, dst);
        assert_eq!(out.len(), 24_000);

        let guard = ZERO_CROSSINGS + 8;
        let mut max_err = 0.0f32;
        for i in guard..out.len() - guard {
            let want = (2.0 * PI * freq * i as f32 / dst as f32).sin();
            max_err = max_err.max((out[i] - want).abs());
        }
        assert!(max_err < 1e-2, "upsampled sine mismatch: max_err {max_err}");
    }

    /// Hygiene: no reachable input produces NaN/Inf. Drive the resampler with a
    /// pseudo-random `[-1, 1]` signal across a sweep of up/down/odd ratios and
    /// assert every output sample is finite.
    #[test]
    fn output_is_finite_across_rate_sweep() {
        // Small LCG so the test is deterministic without a dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((state >> 33) as f32 / (1u32 << 31) as f32) - 1.0 // [-1, 1)
        };
        let input: Vec<f32> = (0..4_000).map(|_| next()).collect();

        for (src, dst) in [
            (48_000, 44_100),
            (44_100, 48_000),
            (96_000, 44_100),
            (44_100, 96_000),
            (48_000, 8_000),
            (22_050, 44_100),
            (48_000, 48_000),
        ] {
            for (i, &y) in resample_channel(&input, src, dst).iter().enumerate() {
                assert!(
                    y.is_finite(),
                    "non-finite output at {src}->{dst}, idx {i}: {y}"
                );
            }
        }
    }
}
