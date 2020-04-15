use super::dsp::*;

/// Voltage Controlled Amplifier
pub struct VCA {
    pub wave: ArcWave,
    pub cv: ArcWave,
}

impl VCA {
    pub fn boxed(wave: ArcWave, cv: ArcWave) -> ArcMutex<Self> {
        arc(VCA { wave, cv })
    }
}

impl Wave for VCA {
    fn sample(&self) -> f32 {
        self.wave.lock().unwrap().sample() * self.cv.lock().unwrap().sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.cv.lock().unwrap().update_phase(sample_rate);
    }
}

/// Voltage Controlled Oscillator
pub struct FMoscillator {
    pub wave: ArcWave,
    pub cv: ArcWave,
    pub mod_idx: Phase,
}

impl FMoscillator {
    pub fn boxed(wave: ArcWave, cv: ArcWave, mod_idx: Phase) -> ArcMutex<Self> {
        arc(FMoscillator { wave, cv, mod_idx })
    }
}

impl Wave for FMoscillator {
    fn sample(&self) -> f32 {
        self.wave.lock().unwrap().sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.cv.lock().unwrap().update_phase(sample_rate);
    }

    //TODO: impl FM
}

pub struct TriggeredWave {
    pub wave: ArcWave,
    pub attack: f32,
    pub decay: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub clock: f64,
    pub triggered: bool,
}

impl Wave for TriggeredWave {
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

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.clock += 1. / sample_rate;
    }
}

pub_struct!(
    struct ADSRWave {
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
        current_time: f64,
    }
);

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

    fn update_phase(&mut self, sample_rate: f64) {
        self.current_time += 1. / sample_rate;
    }
}
