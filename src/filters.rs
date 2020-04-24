use super::dsp::*;
use std::f64::consts::PI;

pub struct BiquadFilter<W>
where
    W: Signal + Send,
{
    pub wave: ArcMutex<W>,
    pub b1: f64,
    pub b2: f64,
    pub a0: f64,
    pub a1: f64,
    pub a2: f64,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    pub off: bool,
}

// See "Audio Processes, Musical Analysis, Modification, Synthesis, and Control"
// by David Creasy, 2017. pages 164-183.
pub fn lpf(sample_rate: f64, fc: Hz, q: f64) -> (f64, f64, f64, f64, f64) {
    let phi = TAU64 * fc / sample_rate;
    let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
    let b1 = -(1.0 + b2) * phi.cos();
    let a0 = 0.25 * (1.0 + b1 + b2);
    let a1 = 2.0 * a0;
    let a2 = a0;
    (b1, b2, a0, a1, a2)
}

pub fn hpf(sample_rate: f64, fc: Hz, q: f64) -> (f64, f64, f64, f64, f64) {
    let phi = TAU64 * fc / sample_rate;
    let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
    let b1 = -(1.0 + b2) * phi.cos();
    let a0 = 0.25 * (1.0 - b1 + b2);
    let a1 = -2.0 * a0;
    let a2 = a0;
    (b1, b2, a0, a1, a2)
}

pub fn lphpf(sample_rate: f64, fc: Hz, q: f64, t: f64) -> (f64, f64, f64, f64, f64) {
    let (b1, b2, a0l, a1l, a2l) = lpf(sample_rate, fc, q);
    let (_, _, a0h, a1h, a2h) = hpf(sample_rate, fc, q);
    (
        b1,
        b2,
        t * a0l + (1. - t) * a0h,
        t * a1l + (1. - t) * a1h,
        t * a2l + (1. - t) * a2h,
    )
}

pub fn bpf(sample_rate: f64, fc: Hz, q: f64) -> (f64, f64, f64, f64, f64) {
    let phi = TAU64 * fc / sample_rate;
    let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
    let b1 = -(1.0 + b2) * phi.cos();
    let a0 = 0.5 * (1.0 - b2);
    let a1 = 0.0;
    let a2 = -a0;
    (b1, b2, a0, a1, a2)
}

pub fn notch(sample_rate: f64, fc: Hz, q: f64) -> (f64, f64, f64, f64, f64) {
    let phi = TAU64 * fc / sample_rate;
    let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
    let b1 = -(1.0 + b2) * phi.cos();
    let a0 = 0.5 * (1.0 + b2);
    let a1 = b1;
    let a2 = a0;
    (b1, b2, a0, a1, a2)
}

impl<W> BiquadFilter<W>
where
    W: Signal + Send,
{
    pub fn new(wave: ArcMutex<W>, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            wave,
            b1,
            b2,
            a0,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn wrapped(
        wave: ArcMutex<W>,
        b1: f64,
        b2: f64,
        a0: f64,
        a1: f64,
        a2: f64,
    ) -> ArcMutex<Self> {
        arc(Self::new(wave, b1, b2, a0, a1, a2))
    }

    pub fn lpf(wave: ArcMutex<W>, sample_rate: f64, fc: Hz, q: f64) -> Self {
        let (b1, b2, a0, a1, a2) = lpf(sample_rate, fc, q);
        Self::new(wave, b1, b2, a0, a1, a2)
    }

    pub fn hpf(wave: ArcMutex<W>, sample_rate: f64, fc: Hz, q: f64) -> Self {
        let (b1, b2, a0, a1, a2) = hpf(sample_rate, fc, q);
        Self::new(wave, b1, b2, a0, a1, a2)
    }

    pub fn lphpf(wave: ArcMutex<W>, sample_rate: f64, fc: Hz, q: f64, t: f64) -> Self {
        let (b1, b2, a0, a1, a2) = lphpf(sample_rate, fc, q, t);
        Self::new(wave, b1, b2, a0, a1, a2)
    }

    pub fn bpf(wave: ArcMutex<W>, sample_rate: f64, fc: Hz, q: f64) -> Self {
        let (b1, b2, a0, a1, a2) = bpf(sample_rate, fc, q);
        Self::new(wave, b1, b2, a0, a1, a2)
    }

    pub fn notch(wave: ArcMutex<W>, sample_rate: f64, fc: Hz, q: f64) -> Self {
        let (b1, b2, a0, a1, a2) = notch(sample_rate, fc, q);
        Self::new(wave, b1, b2, a0, a1, a2)
    }
}

impl<W> Signal for BiquadFilter<W>
where
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let x0 = self.wave.lock().unwrap().signal_(sample_rate, add);
        if self.off {
            return x0;
        };
        let a0 = self.a0 as f32;
        let a1 = self.a1 as f32;
        let a2 = self.a2 as f32;
        let b1 = self.b1 as f32;
        let b2 = self.b2 as f32;
        let amp = a0 * x0 + a1 * self.x1 + a2 * self.x2 - b1 * self.y1 - b2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = amp;
        amp
    }
}
