use super::graph::*;
use std::any::Any;

#[derive(Clone)]
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

    pub fn wrapped(wave: Tag) -> ArcMutex<Self> {
        arc(Self::new(wave))
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
pub fn set_attack(graph: &Graph, n: Tag, a: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.attack = a;
    }
}

pub fn set_decay(graph: &Graph, n: Tag, d: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.decay = d;
    }
}

pub fn set_release(graph: &Graph, n: Tag, r: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.release = r;
    }
}

pub fn set_sustain_level(graph: &Graph, n: Tag, s: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.sustain_level = s;
    }
}

pub fn on(graph: &Graph, n: Tag) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.on();
    }
}

pub fn off(graph: &Graph, n: Tag) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<SustainSynth>()
    {
        v.off();
    }
}
