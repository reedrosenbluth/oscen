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
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        self.carrier.lock().unwrap().signal_add(sample_rate, add)
            * self.modulator.lock().unwrap().signal_add(sample_rate, add)
    }
}

/// Frequency Modulated Oscillator
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
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let m = self.modulator.lock().unwrap().signal_add(sample_rate, add);
        self.carrier
            .lock()
            .unwrap()
            .signal_add(sample_rate, m as f64)
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
        clock: f64,
        triggered: bool,
        level: f32,
    ) -> Self {
        Self {
            wave,
            attack,
            decay,
            sustain_level,
            release,
            clock,
            triggered,
            level,
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
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let amp = self.wave.lock().unwrap().signal_add(sample_rate, add) * self.calc_level();
        self.clock += 1. / sample_rate;
        self.level = self.calc_level();
        amp
    }
}

pub struct ADSRSynth {
    pub attack: f32,
    pub decay: f32,
    pub sustain_time: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub current_time: f64,
}

impl ADSRSynth {
    pub fn new(
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
    ) -> Self {
        ADSRSynth {
            attack: attack,
            decay: decay,
            sustain_time: sustain_time,
            sustain_level: sustain_level,
            release: release,
            current_time: 0.,
        }
    }

    fn adsr(&self, t: f32) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let s = self.sustain_time;
        let r = self.release;
        let sl = self.sustain_level;
        match t {
            x if x < a => t / a,
            x if x < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
            x if x < a + d + s => sl,
            x if x < a + d + s + r => sl - (t - a - d - s) * sl / r,
            _ => 0.0,
        }
    }
}

impl Signal for ADSRSynth {
    fn signal_add(&mut self, sample_rate: f64, _add: Phase) -> Amp {
        let amp = self.adsr(self.current_time as f32);
        self.current_time += 1. / sample_rate;
        amp
    }
}
pub struct LPF<W>
where
    W: Signal + Send,
{
    pub wave: ArcMutex<W>,
    pub cutoff: f32,
    prev_wave_sample: f32,
    prev_sample: f32,
}

impl<W> LPF<W>
where
    W: Signal + Send,
{
    pub fn new(wave: ArcMutex<W>, cutoff: f32) -> Self {
        Self {
            wave,
            cutoff,
            prev_wave_sample: 0.0,
            prev_sample: 0.0,
        }
    }

    pub fn wrapped(wave: ArcMutex<W>, cutoff: f32) -> ArcMutex<Self> {
        arc(Self::new(wave, cutoff))
    }
}

impl<W> Signal for LPF<W>
where
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let wave_sample = self.wave.lock().unwrap().signal_add(sample_rate, add);
        let amp = (1.0 - self.cutoff) * self.prev_sample
            + 0.5 * self.cutoff * (wave_sample + self.prev_wave_sample);
        self.prev_wave_sample = wave_sample;
        amp
    }
}
