//! Offline rendering generalized over the frame type: an all-`Frame<2>` graph
//! (top-level `input stream`/`output stream` typed `Frame<2>`) rendered through
//! `BlockRender::render_mono` reproduces, frame-for-frame, the same output its
//! realtime `process()` path produces for the same input.
#![feature(inherent_associated_types)]

use float_cmp::approx_eq;
use oscen::prelude::*;
use oscen::Node;

const RATE: f32 = 48_000.0;

/// A stereo gain: scales each channel of a `Frame<2>` by a scalar gain.
#[derive(Debug, Node)]
pub struct StereoGain {
    #[input(stream)]
    pub inp: Frame<2>,
    #[output(stream)]
    pub out: Frame<2>,
    gain: f32,
}

impl StereoGain {
    pub fn new(gain: f32) -> Self {
        Self {
            inp: Frame([0.0; 2]),
            out: Frame([0.0; 2]),
            gain,
        }
    }
}

impl Default for StereoGain {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SignalProcessor for StereoGain {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.inp * self.gain;
    }
}

graph! {
    name: StereoGainGraph;

    input stream dry: Frame<2>;
    output stream wet: Frame<2>;

    nodes {
        g = StereoGain::new(0.5);
    }

    connections {
        dry -> g.inp;
        g.out -> wet;
    }
}

/// Deterministic pseudo-noise in [-1, 1] (LCG; no rand dependency).
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed
        .wrapping_mul(2862933555777941757)
        .wrapping_add(3037000493);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        })
        .collect()
}

#[test]
fn stereo_offline_render_matches_realtime() {
    // Distinct per-channel input so a channel-swap/zeroing bug cannot pass.
    let left = noise(2000, 1);
    let right = noise(2000, 2);
    let input: Vec<Frame<2>> = left
        .iter()
        .zip(&right)
        .map(|(&l, &r)| Frame([l, r]))
        .collect();

    // Realtime reference: drive the graph one sample at a time.
    let mut realtime = StereoGainGraph::new();
    realtime.init(RATE);
    let mut want = Vec::with_capacity(input.len());
    for &f in &input {
        realtime.dry = f;
        realtime.process();
        want.push(realtime.wet);
    }

    // Offline: render the whole buffer in one call (spans multiple blocks).
    let mut offline = StereoGainGraph::new();
    offline.init(RATE);
    let got = offline.render_mono(&input, 0);

    assert_eq!(got.len(), want.len(), "frame count mismatch");
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!(
            approx_eq!(f32, g.0[0], w.0[0], epsilon = 1e-6)
                && approx_eq!(f32, g.0[1], w.0[1], epsilon = 1e-6),
            "frame {i}: offline {g:?} != realtime {w:?}"
        );
    }
}
