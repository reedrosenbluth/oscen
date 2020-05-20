use super::graph::*;
use std::any::Any;

pub struct Product {
    pub waves: Vec<Tag>,
}

impl Product {
    pub fn new(waves: Vec<Tag>) -> Self {
        Product { waves }
    }

    pub fn wrapped(waves: Vec<Tag>) -> ArcMutex<Self> {
        arc(Product::new(waves))
    }
}

impl Signal for Product {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        self.waves.iter().fold(1.0, |acc, n| acc * graph.output(*n))
    }
}

pub struct Sum {
    pub waves: Vec<Tag>,
}

impl Sum {
    pub fn new(waves: Vec<Tag>) -> Self {
        Sum { waves }
    }

    pub fn wrapped(waves: Vec<Tag>) -> ArcMutex<Self> {
        arc(Sum::new(waves))
    }
}

impl Signal for Sum {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        self.waves.iter().fold(0.0, |acc, n| acc + graph.output(*n))
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

    pub fn wrapped(wave1: Tag, wave2: Tag) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2))
    }
}

impl Signal for Lerp {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let alpha = In::val(graph, self.alpha);
        alpha * graph.output(self.wave2) + (1.0 - alpha) * graph.output(self.wave1)
    }
}

pub struct Lerp3 {
    pub lerp1: Tag,
    pub lerp2: Tag,
    pub knob: In,
}

impl Lerp3 {
    pub fn new(lerp1: Tag, lerp2: Tag, knob: In) -> Self {
        Self { lerp1, lerp2, knob }
    }

    pub fn wrapped(lerp1: Tag, lerp2: Tag, knob: In) -> ArcMutex<Self> {
        arc(Self::new(lerp1, lerp2, knob))
    }

    pub fn set_alphas(&mut self, graph: &Graph) {
        let knob = In::val(graph, self.knob);
        if In::val(graph, self.knob) <= 0.5 {
            if let Some(a) = graph.nodes[self.lerp1]
                .module
                .lock()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Lerp>()
            {
                a.alpha = fix(2.0 * knob);
            }
            if let Some(a) = graph.nodes[self.lerp2]
                .module
                .lock()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Lerp>()
            {
                a.alpha = fix(0.0);
            }
        } else {
            if let Some(a) = graph.nodes[self.lerp1]
                .module
                .lock()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Lerp>()
            {
                a.alpha = fix(0.0);
            }
            if let Some(a) = graph.nodes[self.lerp2]
                .module
                .lock()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Lerp>()
            {
                a.alpha = fix(2.0 * (knob - 0.5));
            }
        }
    }
}

impl Signal for Lerp3 {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        self.set_alphas(graph);
        if In::val(graph, self.knob) <= 0.5 {
            graph.output(self.lerp1)
        } else {
            graph.output(self.lerp2)
        }
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
