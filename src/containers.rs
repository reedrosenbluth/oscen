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
    fn signal(&mut self, sample_rate: f64) -> Amp {
        self.carrier.mtx().signal(sample_rate) * self.modulator.mtx().signal(sample_rate)
    }
}

impl<V, W> HasHz for RMSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + Send,
{
    fn hz(&self) -> Hz {
        self.carrier.mtx().hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.carrier.mtx().modify_hz(f)
    }
}

pub struct FMSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + Send,
{
    pub carrier: ArcMutex<V>,
    pub modulator: ArcMutex<W>,
    pub base_hz: Hz,
    pub mod_idx: Phase,
}

impl<V, W> FMSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + Send,
{
    pub fn new(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> Self {
        // set the base frequencey to the carrier frequency
        let base_hz = carrier.mtx().hz();
        Self {
            carrier,
            modulator,
            base_hz,
            mod_idx,
        }
    }

    pub fn wrapped(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> ArcMutex<Self> {
        arc(FMSynth::new(carrier, modulator, mod_idx))
    }
}

impl<V, W> Signal for FMSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let mod_hz = self.modulator.mtx().hz();
        let m = self.mod_idx * mod_hz * self.modulator.mtx().signal(sample_rate) as f64;
        self.carrier.mtx().set_hz(self.base_hz + m);
        self.carrier.mtx().signal(sample_rate)
    }
}

impl<V, W> HasHz for FMSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + Send,
{
    fn hz(&self) -> Hz {
        self.carrier.mtx().hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        let hz = f(self.base_hz);
        self.base_hz = hz;
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
    W: Signal + HasHz + Send,
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
    W: Signal + HasHz + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let amp = self.wave.mtx().signal(sample_rate) * self.calc_level();
        self.clock += 1. / sample_rate;
        self.level = self.calc_level();
        amp
    }
}

impl<W> HasHz for SustainSynth<W>
where
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        self.wave.mtx().hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave.mtx().modify_hz(f)
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
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let level = self.adsr();
        let amp = self.wave.mtx().signal(sample_rate) * level;
        self.clock += 1. / sample_rate;
        self.level = level;
        amp
    }
}

impl<W> HasHz for TriggerSynth<W>
where
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        self.wave.mtx().hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave.mtx().modify_hz(f)
    }
}