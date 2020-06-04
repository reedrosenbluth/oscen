use super::graph::*;
use crate::{std_signal, as_any_mut, tag, impl_set};
use std::{
    any::Any,
    ops::{Index, IndexMut},
};

#[derive(Clone)]
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
    pub fn new(attack: Real, decay: Real, sustain: Real, release: Real) -> Self {
        Self {
            tag: mk_tag(),
            attack: attack.into(),
            decay: decay.into(),
            sustain: sustain.into(),
            release: release.into(),
            clock: 0.0,
            triggered: false,
            level: 0.0,
        }
    }

    pub fn wrapped(attack: Real, decay: Real, sustain: Real, release: Real) -> ArcMutex<Self> {
        arc(Self::new(attack, decay, sustain, release))
    }

    pub fn calc_level(&self, graph: &Graph) -> Real {
        fn max01(a: f64) -> f64 {
            if a > 0.01 { a } else { 0.01 }
        }

        let a = max01(In::val(graph, self.attack));
        let d = max01(In::val(graph, self.decay));
        let s = In::val(graph, self.sustain);
        let r = max01(In::val(graph, self.release));

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

    pub fn on(&mut self, graph: &Graph) {
        self.triggered = true;
        self.clock = self.level * In::val(graph, self.attack);
    }

    pub fn off(&mut self, graph: &Graph) {
        let s = In::val(graph, self.sustain);
        let r = In::val(graph, self.release);
        self.triggered = false;
        self.clock = (s - self.level) * r / s;
    }
}

impl Signal for Adsr {
    std_signal!();
    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let amp = self.calc_level(graph);
        self.clock += 1. / sample_rate;
        self.level = self.calc_level(graph);
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

pub fn on(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Adsr>()
    {
        v.on(graph);
    }
}

pub fn off(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Adsr>()
    {
        v.off(graph);
    }
}
