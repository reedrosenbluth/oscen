use super::dsp::*;

/// Voltage Controlled Amplifier
pub struct VCA<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub carrier: ArcMutex<V>,
    pub modulator: ArcMutex<W>,
}

impl<V, W> VCA<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(carrier: ArcMutex<V>, modulator: ArcMutex<W>) -> Self {
        Self { carrier, modulator }
    }

    pub fn boxed(carrier: ArcMutex<V>, modulator: ArcMutex<W>) -> ArcMutex<Self> {
        arc(VCA { carrier, modulator })
    }
}

impl<V, W> Wave for VCA<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        self.carrier.lock().unwrap().sample() * self.modulator.lock().unwrap().sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.carrier.lock().unwrap().update_phase(0.0, sample_rate);
        self.modulator
            .lock()
            .unwrap()
            .update_phase(0.0, sample_rate);
    }
}

/// Voltage Controlled Oscillator
pub struct FMoscillator<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub carrier: ArcMutex<V>,
    pub modulator: ArcMutex<W>,
    pub mod_idx: Phase,
}

impl<V, W> FMoscillator<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> Self {
        Self {
            carrier,
            modulator,
            mod_idx,
        }
    }

    pub fn boxed(carrier: ArcMutex<V>, modulator: ArcMutex<W>, mod_idx: Phase) -> ArcMutex<Self> {
        arc(FMoscillator {
            carrier,
            modulator,
            mod_idx,
        })
    }
}

impl<V, W> Wave for FMoscillator<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        self.carrier.lock().unwrap().sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        let m = self.mod_idx as f32 * self.modulator.lock().unwrap().sample();
        self.carrier
            .lock()
            .unwrap()
            .update_phase(m as f64, sample_rate);
        self.modulator
            .lock()
            .unwrap()
            .update_phase(0.0, sample_rate);
    }
}

pub struct TriggeredWave<W>
where
    W: Wave + Send,
{
    pub wave: ArcMutex<W>,
    pub attack: f32,
    pub decay: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub clock: f64,
    pub triggered: bool,
}

impl<W> Wave for TriggeredWave<W>
where
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let r = self.release;
        let sl = self.sustain_level;
        let level = if self.triggered {
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
        };
        self.wave.lock().unwrap().sample() * level
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(0.0, sample_rate);
        self.clock += 1. / sample_rate;
    }
}

pub struct ADSRWave {
    pub attack: f32,
    pub decay: f32,
    pub sustain_time: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub current_time: f64,
}

impl ADSRWave {
    pub fn new(
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
    ) -> Self {
        ADSRWave {
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

impl Wave for ADSRWave {
    fn sample(&self) -> f32 {
        self.adsr(self.current_time as f32)
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.current_time += 1. / sample_rate;
    }
}
