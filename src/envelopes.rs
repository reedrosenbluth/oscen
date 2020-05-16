use super::graph::*;
use std::any::Any;

pub struct SustainSynth {
    pub wave: Tag,
    pub attack: Real,
    pub decay: Real,
    pub sustain_level: Real,
    pub release: Real,
    pub clock: Real,
    pub triggered: bool,
    pub level: Real,
}

impl SustainSynth {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            attack: 0.2,
            decay: 0.1,
            sustain_level: 0.8,
            release: 0.2,
            clock: 0.0,
            triggered: false,
            level: 0.0,
        }
    }

    pub fn calc_level(&self) -> Real {
        let a = self.attack;
        let d = self.decay;
        let r = self.release;
        let sl = self.sustain_level;
        if self.triggered {
            match self.clock {
                t if t < a => t / a,
                t if t < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
                _ => sl,
            }
        } else {
            match self.clock {
                t if t < r => sl - t / r * sl,
                _ => 0.,
            }
        }
    }

    pub fn on(&mut self) {
        self.clock = self.level * self.attack;
        self.triggered = true;
    }

    pub fn off(&mut self) {
        self.clock = (self.sustain_level - self.level) * self.release / self.sustain_level;
        self.triggered = false;
    }
}

impl Signal for SustainSynth {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let amp = graph.output(self.wave) * self.calc_level();
        self.clock += 1. / sample_rate;
        self.level = self.calc_level();
        amp
    }
}
