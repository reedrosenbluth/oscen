use super::signal::*;
use crate::{std_signal, as_any_mut, impl_set};
use std::{
    any::Any,
    ops::{Index, IndexMut},
};

#[derive(Copy, Clone)]
pub struct Adsr {
    pub tag: Tag,
    pub attack: In,
    pub decay: In,
    pub sustain: In,
    pub release: In,
    clock: Real,
    pub triggered: bool,
    level: Real,
}

impl Adsr {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            attack: (0.01).into(),
            decay: In::zero(),
            sustain: In::one(),
            release: (0.1).into(),
            clock: 0.0,
            triggered: false,
            level: 0.0,
        }
    }

    pub fn attack(&mut self, arg: In) -> &mut Self {
        self.attack = arg;
        self
    }

    pub fn decay(&mut self, arg: In) -> &mut Self {
        self.decay = arg;
        self
    }
    
    pub fn sustain(&mut self, arg: In) -> &mut Self {
        self.sustain = arg;
        self
    }

    pub fn release(&mut self, arg: In) -> &mut Self {
        self.release = arg;
        self
    }

    pub fn calc_level(&self, rack: &Rack) -> Real {
        fn max01(a: f64) -> f64 {
            if a > 0.01 { a } else { 0.01 }
        }

        let a = max01(In::val(rack, self.attack));
        let d = max01(In::val(rack, self.decay));
        let s = In::val(rack, self.sustain);
        let r = max01(In::val(rack, self.release));

        if self.triggered {
            match self.clock {
                // Attack
                t if t < a => t / a,
                // Decay
                t if t < a + d => 1.0 + (t - a) * (s - 1.0) / d,
                // Sustain
                _ => s,
            }
        } else {
            match self.clock {
                // Release
                t if t < r => s - t / r * s,
                // Off
                _ => 0.,
            }
        }
    }

    pub fn on(&mut self, rack: &Rack) {
        self.triggered = true;
        self.clock = self.level * In::val(rack, self.attack);
    }

    pub fn off(&mut self, rack: &Rack) {
        let s = In::val(rack, self.sustain);
        let r = In::val(rack, self.release);
        self.triggered = false;
        self.clock = (s - self.level) * r / s;
    }
}

impl Builder for Adsr {}

impl Signal for Adsr {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let amp = self.calc_level(rack);
        self.clock += 1. / sample_rate;
        self.level = self.calc_level(rack);
        amp
    }
}

impl Index<&str> for Adsr {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "attack" => &self.attack,
            "decay" => &self.decay,
            "sustain" => &self.sustain,
            "release" => &self.release,
            _ => panic!("Adsr does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Adsr {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "attack" => &mut self.attack,
            "decay" => &mut self.decay,
            "sustain" => &mut self.sustain,
            "release" => &mut self.release,
            _ => panic!("Adsr does not have a field named: {}", index),
        }
    }
}

impl_set!(Adsr);

pub fn on(rack: &Rack, n: Tag) {
    if let Some(v) = rack.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Adsr>()
    {
        v.on(rack);
    }
}

pub fn off(rack: &Rack, n: Tag) {
    if let Some(v) = rack.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Adsr>()
    {
        v.off(rack);
    }
}
