use oscen::resample::{LatchDown, LatchUp, StreamDownsampler, StreamUpsampler};

#[test]
fn latch_up_2_holds_value() {
    let mut up = LatchUp::<2>::new();
    let mut out = [0.0_f32; 2];
    up.upsample(1.0, &mut out);
    assert_eq!(out, [1.0, 1.0]);
    up.upsample(-0.25, &mut out);
    assert_eq!(out, [-0.25, -0.25]);
}

#[test]
fn latch_up_4_holds_value() {
    let mut up = LatchUp::<4>::new();
    let mut out = [0.0_f32; 4];
    up.upsample(0.5, &mut out);
    assert_eq!(out, [0.5; 4]);
}

#[test]
fn latch_up_zero_latency() {
    assert_eq!(LatchUp::<2>::new().latency_samples(), 0);
    assert_eq!(LatchUp::<4>::new().latency_samples(), 0);
    assert_eq!(LatchUp::<8>::new().latency_samples(), 0);
}

#[test]
fn latch_down_2_takes_first() {
    let mut down = LatchDown::<2>::new();
    let y = down.downsample(&[1.0, 2.0]);
    assert_eq!(y, 1.0);
    let y = down.downsample(&[3.0, 4.0]);
    assert_eq!(y, 3.0);
}

#[test]
fn latch_down_4_takes_first() {
    let mut down = LatchDown::<4>::new();
    let y = down.downsample(&[10.0, 11.0, 12.0, 13.0]);
    assert_eq!(y, 10.0);
}

#[test]
fn latch_down_zero_latency() {
    assert_eq!(LatchDown::<2>::new().latency_samples(), 0);
    assert_eq!(LatchDown::<4>::new().latency_samples(), 0);
    assert_eq!(LatchDown::<8>::new().latency_samples(), 0);
}

#[test]
fn latch_reset_is_noop() {
    let mut up = LatchUp::<2>::new();
    up.reset();
    let mut out = [0.0; 2];
    up.upsample(1.0, &mut out);
    assert_eq!(out, [1.0, 1.0]);
}

use oscen::resample::{LinearDown, LinearUp};
use float_cmp::approx_eq;

#[test]
fn linear_up_2_interpolates_between_samples() {
    let mut up = LinearUp::<2>::new();
    let mut out = [0.0_f32; 2];

    // Sentinel definition: out[i] = prev + (current - prev) * (i / N) for i in 0..N.
    up.upsample(0.0, &mut out);
    up.upsample(1.0, &mut out);
    // After feeding 0.0 then 1.0, out should be [0.0, 0.5]
    assert!(approx_eq!(f32, out[0], 0.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 0.5, epsilon = 1e-6));

    up.upsample(2.0, &mut out);
    // prev=1.0, current=2.0, N=2: out[0] = 1.0, out[1] = 1.5.
    assert!(approx_eq!(f32, out[0], 1.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 1.5, epsilon = 1e-6));
}

#[test]
fn linear_up_dc_passes_through_after_warmup() {
    let mut up = LinearUp::<4>::new();
    let mut out = [0.0_f32; 4];
    // After two consecutive identical inputs, the output should be the constant.
    up.upsample(0.7, &mut out);
    up.upsample(0.7, &mut out);
    for v in out {
        assert!(approx_eq!(f32, v, 0.7, epsilon = 1e-6));
    }
}

#[test]
fn linear_up_latency_is_one_dest_sample() {
    assert_eq!(LinearUp::<2>::new().latency_samples(), 1);
    assert_eq!(LinearUp::<4>::new().latency_samples(), 1);
}

#[test]
fn linear_down_2_averages_pair() {
    let mut down = LinearDown::<2>::new();
    let y = down.downsample(&[1.0, 3.0]);
    assert!(approx_eq!(f32, y, 2.0, epsilon = 1e-6));
}

#[test]
fn linear_down_4_averages() {
    let mut down = LinearDown::<4>::new();
    let y = down.downsample(&[1.0, 2.0, 3.0, 4.0]);
    assert!(approx_eq!(f32, y, 2.5, epsilon = 1e-6));
}

#[test]
fn linear_reset_clears_history() {
    let mut up = LinearUp::<2>::new();
    let mut out = [0.0_f32; 2];
    up.upsample(1.0, &mut out);
    up.reset();
    up.upsample(1.0, &mut out);
    // After reset prev should be 0 again, so out[0] = 0.0, out[1] = 0.5
    assert!(approx_eq!(f32, out[0], 0.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 0.5, epsilon = 1e-6));
}

use oscen::resample::{SincDownFir, SincUpFir};

fn db(x: f32) -> f32 { 20.0 * x.abs().max(1e-12).log10() }

#[test]
fn sinc_fir_up_dc_unity_gain() {
    let mut up = SincUpFir::<2>::new();
    let mut out = [0.0_f32; 2];
    let mut last = [0.0_f32; 2];
    for _ in 0..200 {
        up.upsample(0.7, &mut out);
        last = out;
    }
    assert!(approx_eq!(f32, last[0], 0.7, epsilon = 1e-3));
    assert!(approx_eq!(f32, last[1], 0.7, epsilon = 1e-3));
}

#[test]
fn sinc_fir_down_dc_unity_gain() {
    let mut down = SincDownFir::<2>::new();
    let mut y = 0.0;
    for _ in 0..200 {
        y = down.downsample(&[0.7, 0.7]);
    }
    assert!(approx_eq!(f32, y, 0.7, epsilon = 1e-3));
}

#[test]
fn sinc_fir_passband_flat() {
    let mut up = SincUpFir::<2>::new();
    let mut down = SincDownFir::<2>::new();
    let mut buf = [0.0_f32; 2];
    let f = 0.1;
    let mut max_err = 0.0_f32;
    let total = 1024;
    let warmup = 64;
    // Latencies are reported at the high (2×) rate; sum at high rate then
    // convert to low-rate samples. Per-side floor division would lose a sample
    // (5 + 5 = 10) versus the true round-trip 22-high-rate / 2 = 11 low-rate.
    let up_lat = SincUpFir::<2>::new().latency_samples();
    let down_lat = SincDownFir::<2>::new().latency_samples();
    let lag = (up_lat + down_lat) / 2;
    for n in 0..total {
        let x = (2.0 * std::f32::consts::PI * f * n as f32).sin();
        up.upsample(x, &mut buf);
        let y = down.downsample(&buf);
        if n > warmup && n >= lag {
            let expected = (2.0 * std::f32::consts::PI * f * (n - lag) as f32).sin();
            max_err = max_err.max((y - expected).abs());
        }
    }
    assert!(max_err < 0.1, "max passband error = {max_err}");
}

#[test]
fn sinc_fir_stopband_attenuated() {
    // Drive the downsampler directly with a high-rate signal whose frequency lies
    // in the halfband stopband (above 0.5 normalized to high-rate Nyquist, i.e.
    // f > 0.25 in high-rate cycles/sample). This actually exercises the halfband
    // attenuation. An up→down cascade can't be used to test the stopband because
    // any low-rate input above low-rate Nyquist (which is what feeding f > 0.5 to
    // the upsampler implies) is already aliased before reaching the filter.
    let mut down = SincDownFir::<2>::new();
    let mut peak = 0.0_f32;
    let f = 0.4; // high-rate cycles/sample, above halfband cutoff (0.25)
    let warmup = 128;
    for m in 0..2048 {
        let x0 = (2.0 * std::f32::consts::PI * f * (2 * m) as f32).sin();
        let x1 = (2.0 * std::f32::consts::PI * f * (2 * m + 1) as f32).sin();
        let y = down.downsample(&[x0, x1]);
        if m > warmup {
            peak = peak.max(y.abs());
        }
    }
    let attenuation_db = -db(peak);
    assert!(attenuation_db > 50.0, "stopband attenuation = {attenuation_db} dB");
}

#[test]
fn sinc_fir_latency_matches_const() {
    assert!(SincUpFir::<2>::new().latency_samples() > 0);
    assert!(SincUpFir::<4>::new().latency_samples() >= SincUpFir::<2>::new().latency_samples());
    assert!(SincUpFir::<8>::new().latency_samples() >= SincUpFir::<4>::new().latency_samples());
}

use oscen::resample::{IirHalfbandDown, IirHalfbandUp};

#[test]
fn iir_hb_up_dc_unity_gain() {
    let mut up = IirHalfbandUp::<2>::new();
    let mut out = [0.0_f32; 2];
    let mut last = [0.0_f32; 2];
    for _ in 0..1000 { up.upsample(0.5, &mut out); last = out; }
    assert!(approx_eq!(f32, last[0], 0.5, epsilon = 5e-3));
    assert!(approx_eq!(f32, last[1], 0.5, epsilon = 5e-3));
}

#[test]
fn iir_hb_down_dc_unity_gain() {
    let mut down = IirHalfbandDown::<2>::new();
    let mut y = 0.0;
    for _ in 0..1000 { y = down.downsample(&[0.5, 0.5]); }
    assert!(approx_eq!(f32, y, 0.5, epsilon = 5e-3));
}

#[test]
fn iir_hb_stopband_attenuated() {
    // Drive the IIR halfband downsampler directly with a high-rate signal in the
    // stopband (analogous to sinc_fir_stopband_attenuated's design).
    let mut down = IirHalfbandDown::<2>::new();
    let mut peak = 0.0_f32;
    let f = 0.4; // high-rate cycles/sample, in halfband stopband
    let warmup = 256;
    for m in 0..4096 {
        let x0 = (2.0 * std::f32::consts::PI * f * (2 * m) as f32).sin();
        let x1 = (2.0 * std::f32::consts::PI * f * (2 * m + 1) as f32).sin();
        let y = down.downsample(&[x0, x1]);
        if m > warmup { peak = peak.max(y.abs()); }
    }
    let atten_db = -db(peak);
    assert!(atten_db > 40.0, "IIR halfband stopband = {atten_db} dB");
}

#[test]
fn iir_hb_latency_smaller_than_fir() {
    let iir = IirHalfbandUp::<2>::new().latency_samples();
    let fir = SincUpFir::<2>::new().latency_samples();
    assert!(iir < fir, "IIR halfband should have lower latency than FIR (got {iir} vs {fir})");
}
