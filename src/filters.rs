use super::dsp::*;

pub struct BiquadFilter<W>
where
    W: Signal + Send,
{
    pub wave: ArcMutex<W>,
    pub a1: f32,
    pub a2: f32,
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    pub off: bool,
}

pub fn lpf(sample_rate: f64, cutoff: Hz, q: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = 0.5 * (1.0 - w0.cos());
    let b1 = 2.0 * b0;
    let b2 = b0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}
pub fn hpf(sample_rate: f64, cutoff: Hz, q: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = 0.5 * (1.0 + w0.cos());
    let b1 = -2.0 * b0;
    let b2 = b0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}

pub fn lphpf(sample_rate: f64, cutoff: Hz, q: f32, t: f32) -> (f32, f32, f32, f32, f32) {
    let (a1, a2, b0l, b1l, b2l) = lpf(sample_rate, cutoff, q);
    let (_,  _,  b0h, b1h, b2h) = hpf(sample_rate, cutoff, q);
    (a1, a2, t * b0l + (1.-t) * b0h, t * b1l + (1.-t) * b1h, t * b2l + (1.-t) * b2h)
}

pub fn bpf(sample_rate: f64, cutoff: Hz, q: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = q * alpha;
    let b1 = 0.0;
    let b2 = -b0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}

pub fn notch(sample_rate: f64, cutoff: Hz, q: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = 1.0;
    let b1 = -2.0 * w0.cos();
    let b2 = 1.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}

pub fn apf(sample_rate: f64, cutoff: Hz, q: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = 1.0 - alpha;
    let b1 = -2.0 * w0.cos();
    let b2 = 1.0 + alpha;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}

pub fn peak(sample_rate: f64, cutoff: Hz, q: f32, gain: f32) -> (f32, f32, f32, f32, f32) {
    let cutoff = cutoff as f32;
    let sample_rate = sample_rate as f32;
    let w0 = TAU32 * cutoff / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    let a = ((10.0_f32).powf(gain / 20.0)).sqrt();
    let b0 = 1.0 - alpha * a;
    let b1 = -2.0 * w0.cos();
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha / a;
    (a1 / a0, a2 / a0, b0 / a0, b1 / a0, b2 / a0)
}

impl<W> BiquadFilter<W>
where
    W: Signal + Send,
{
    pub fn new(wave: ArcMutex<W>, a1: f32, a2: f32, b0: f32, b1: f32, b2: f32) -> Self {
        Self {
            wave,
            a1,
            a2,
            b0,
            b1,
            b2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn wrapped(
        wave: ArcMutex<W>,
        a1: f32,
        a2: f32,
        b0: f32,
        b1: f32,
        b2: f32,
    ) -> ArcMutex<Self> {
        arc(Self::new(wave, a1, a2, b0, b1, b2))
    }

    // The following functions are based on:
    // https://www.w3.org/2011/audio/audio-eq-cookbook.html

    pub fn lpf(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32) -> Self {
        let (a1, a2, b0, b1, b2) = lpf(sample_rate, cutoff, q);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn hpf(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32) -> Self {
        let (a1, a2, b0, b1, b2) = hpf(sample_rate, cutoff, q);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn lphpf(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32, t: f32) -> Self {
        let (a1, a2, b0, b1, b2) = lphpf(sample_rate, cutoff, q, t);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn bpf(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32) -> Self {
        let (a1, a2, b0, b1, b2) = bpf(sample_rate, cutoff, q);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn notch(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32) -> Self {
        let (a1, a2, b0, b1, b2) = notch(sample_rate, cutoff, q);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn apf(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32) -> Self {
        let (a1, a2, b0, b1, b2) = apf(sample_rate, cutoff, q);
        Self::new(wave, a1, a2, b0, b1, b2)
    }

    pub fn peak(wave: ArcMutex<W>, sample_rate: f64, cutoff: Hz, q: f32, gain: f32) -> Self {
        let (a1, a2, b0, b1, b2) = peak(sample_rate, cutoff, q, gain);
        Self::new(wave, a1, a2, b0, b1, b2)
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
        let amp = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = amp;
        amp
    }
}
