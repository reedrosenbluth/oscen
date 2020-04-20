use super::dsp::*;

/// Ring Modulation
pub struct RMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub carrier: ArcMutex<V>,
    pub modulator: ArcMutex<W>,
}

impl<V, W> RMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(carrier: ArcMutex<V>, modulator: ArcMutex<W>) -> Self {
        Self { carrier, modulator }
    }

    pub fn wrapped(carrier: ArcMutex<V>, modulator: ArcMutex<W>) -> ArcMutex<Self> {
        arc(RMSynth { carrier, modulator })
    }
}

impl<V, W> Signal for RMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        self.carrier.lock().unwrap().signal_(sample_rate, add)
            * self.modulator.lock().unwrap().signal_(sample_rate, add)
    }
}

/// Frequency Modulated Oscillator ala Yamaha DX7. Technically phase modulation.
pub struct FMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub carrier: ArcMutex<V>,
    pub modulator: ArcMutex<W>,
    pub mod_idx: Phase,
}

impl<V, W> FMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> Self {
        Self {
            carrier,
            modulator,
            mod_idx,
        }
    }

    pub fn wrapped(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> ArcMutex<Self> {
        arc(FMSynth {
            carrier,
            modulator,
            mod_idx,
        })
    }
}

impl<V, W> Signal for FMSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let m = self.modulator.lock().unwrap().signal_(sample_rate, add);
        self.carrier.lock().unwrap().signal_(sample_rate, m as f64)
    }
}

pub struct SustainSynth<W>
where
    W: Signal + Send,
{
    pub wave: ArcMutex<W>,
    pub attack: f32,
    pub decay: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub clock: f64,
    pub triggered: bool,
    pub level: f32,
}

impl<W> SustainSynth<W>
where
    W: Signal + Send,
{
    pub fn new(
        wave: ArcMutex<W>,
        attack: f32,
        decay: f32,
        sustain_level: f32,
        release: f32,
    ) -> Self {
        Self {
            wave,
            attack,
            decay,
            sustain_level,
            release,
            clock: 0.0,
            triggered: false,
            level: 0.0,
        }
    }

    pub fn calc_level(&self) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let r = self.release;
        let sl = self.sustain_level;
        if self.triggered {
            match self.clock as f32 {
                t if t < a => t / a,
                t if t < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
                _ => sl,
            }
        } else {
            match self.clock as f32 {
                t if t < r => sl - t / r * sl,
                _ => 0.,
            }
        }
    }

    pub fn on(&mut self) {
        self.clock = self.level as f64 * self.attack as f64;
        self.triggered = true;
    }

    pub fn off(&mut self) {
        self.clock = 0.0;
        self.triggered = false;
    }
}

impl<W> Signal for SustainSynth<W>
where
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let amp = self.wave.lock().unwrap().signal_(sample_rate, add) * self.calc_level();
        self.clock += 1. / sample_rate;
        self.level = self.calc_level();
        amp
    }
}

pub struct TriggerSynth<W>
where
    W: Signal + Send,
{
    pub wave: ArcMutex<W>,
    pub attack: f32,
    pub decay: f32,
    pub sustain_time: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub clock: f64,
    pub triggered: bool,
    pub level: f32,
}

impl<W> TriggerSynth<W>
where
    W: Signal + Send,
{
    pub fn new(
        wave: ArcMutex<W>,
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
    ) -> Self {
        TriggerSynth {
            wave,
            attack,
            decay,
            sustain_time,
            sustain_level,
            release,
            clock: 0.0,
            triggered: false,
            level: 0.0,
        }
    }

    pub fn adsr(&mut self) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let s = self.sustain_time;
        let r = self.release;
        let sl = self.sustain_level;
        let t = self.clock as f32;
        if self.triggered {
            match t {
                x if x < a => t / a,
                x if x < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
                x if x < a + d + s => sl,
                x if x < a + d + s + r => sl - (t - a - d - s) * sl / r,
                _ => {
                    self.triggered = false;
                    0.0
                }
            }
        } else {
            0.0
        }
    }

    pub fn on(&mut self) {
        self.clock = self.level as f64 * self.attack as f64;
        self.triggered = true;
    }
}

impl<W> Signal for TriggerSynth<W>
where
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let level = self.adsr();
        let amp = self.wave.lock().unwrap().signal_(sample_rate, add) * level;
        self.clock += 1. / sample_rate;
        self.level = level;
        amp
    }
}

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
}

impl<W> Signal for BiquadFilter<W>
where
    W: Signal + Send,
{
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let x0 = self.wave.lock().unwrap().signal_(sample_rate, add);
        let amp = self.b0 * x0
            + self.b1 * self.x1
            + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = amp;
        amp
    }
}
