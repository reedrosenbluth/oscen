use std::{
    any::Any,
    f64::consts::PI,
    sync::{Arc, Mutex},
};

pub const TAU: f64 = 2.0 * PI;
pub type Real = f64;
pub type Tag = usize;

/// Synth modules must implement the Signal trait. `as_any_mut` is necessary
/// so that modules can be downcast in order to access their specific fields.
pub trait Signal: Any {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Responsible for updating the `phase` and returning the next signal
    /// value, i.e. `amplitude`.
    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real;
}

/// Signals typically need to decalare that they are `Send` so that they are
/// thread safe.
pub type Sig = dyn Signal + Send;
pub type ArcMutex<T> = Arc<Mutex<T>>;

/// Convenience function for `Arc<Mutex<...>`.
pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

/// Inputs to synth modules can either be constant (`Fixed`) or modulated by
/// another signal (`Var`).
#[derive(Copy, Clone)]
pub enum In {
    Var(Tag),
    Fixed(Real),
}

impl In {
    /// Get the signal value. Look it up in the graph if it is `Var`.
    pub fn val(graph: &Graph, inp: In) -> Real {
        match inp {
            In::Fixed(x) => x,
            In::Var(n) => graph.output(n),
        }
    }
}

/// Create a modulateable input.
pub fn var(n: Tag) -> In {
    In::Var(n)
}

/// Create a constant input.
pub fn fix(x: Real) -> In {
    In::Fixed(x)
}

/// Nodes for the graph will have both a synth module (i.e an implentor of 
/// `Signal`) and will store there previous signal value as `output`
pub struct Node {
    pub module: ArcMutex<Sig>,
    pub output: Real,
}

impl Node {
    fn new(signal: ArcMutex<Sig>) -> Self {
        Node {
            module: signal,
            output: 0.0,
        }
    }
}

/// A `Graph` is just a vector of nodes to be visited in order.
pub struct Graph(pub Vec<Node>);

impl Graph {
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut ns: Vec<Node> = Vec::new();
        for s in ws {
            ns.push(Node::new(s));
        }
        Graph(ns)
    }

    pub fn output(&self, n: Tag) -> Real {
        self.0[n].output
    }
    /// A `Graph` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        let mut outs: Vec<Real> = Vec::new();
        for node in self.0.iter() {
            outs.push(node.module.lock().unwrap().signal(&self, sample_rate));
        }
        for (i, node) in self.0.iter_mut().enumerate() {
            node.output = outs[i];
        }
        self.0[self.0.len() - 1].output
    }
}
pub struct Lerp {
    wave1: Tag,
    wave2: Tag,
    alpha: In,
}

impl Lerp {
    pub fn new(wave1: Tag, wave2: Tag) -> Self {
        Lerp {
            wave1,
            wave2,
            alpha: fix(0.5),
        }
    }
}

impl Signal for Lerp {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let alpha = In::val(graph, self.alpha);
        alpha * graph.output(self.wave1) + (1.0 - alpha) * graph.output(self.wave2)
    }
}

pub struct Modulator {
    pub wave: Tag,
    pub base_hz: In,
    pub mod_hz: In,
    pub mod_idx: In,
}

impl Modulator {
    pub fn new(wave: Tag, base_hz: Real, mod_hz: Real) -> Self {
        Modulator {
            wave,
            base_hz: fix(base_hz),
            mod_hz: fix(mod_hz),
            mod_idx: fix(1.0),
        }
    }

    pub fn wrapped(wave: Tag, base_hz: Real, mod_hz: Real) -> ArcMutex<Self> {
        arc(Modulator::new(wave, base_hz, mod_hz))
    }
}

impl Signal for Modulator {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let mod_hz = In::val(graph, self.mod_hz);
        let mod_idx = In::val(graph, self.mod_idx);
        let base_hz = In::val(graph, self.base_hz);
        base_hz + mod_idx * mod_hz * graph.output(self.wave)
    }
}

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
