use super::graph::*;
use std::any::Any;
use std::ops::{Index, IndexMut};

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

pub fn set_alpha(graph: &Graph, n: Tag, a: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Lerp>()
    {
        v.alpha = fix(a)
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
            set_alpha(&graph, self.lerp1, 2.0 * knob);
            set_alpha(&graph, self.lerp2, 0.0);
        } else {
            set_alpha(&graph, self.lerp1, 0.0);
            set_alpha(&graph, self.lerp2, 2.0 * (knob - 0.5));
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

pub fn set_knob(graph: &Graph, n: Tag, k: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Lerp3>()
    {
        v.knob = fix(k);
        v.set_alphas(graph);
    }
}

pub struct Modulator {
    pub wave: In,
    pub base_hz: In,
    pub mod_hz: In,
    pub mod_idx: In,
}

impl Modulator {
    pub fn new(wave: Tag, base_hz: Real, mod_hz: Real, mod_idx: Real) -> Self {
        Modulator {
            wave: var(wave),
            base_hz: fix(base_hz),
            mod_hz: fix(mod_hz),
            mod_idx: fix(mod_idx),
        }
    }

    pub fn wrapped(wave: Tag, base_hz: Real, mod_hz: Real, mod_idx: Real) -> ArcMutex<Self> {
        arc(Modulator::new(wave, base_hz, mod_hz, mod_idx))
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
        base_hz + mod_idx * mod_hz *  In::val(graph, self.wave)
    }
}

impl Index<&str> for Modulator {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "wave" => &self.wave,
            "base_hz" => &self.base_hz,
            "mod_hz" => &self.mod_hz,
            "mod_idx" => &self.mod_idx,
            _ => panic!("Modulator only does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Modulator {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "wave" => &mut self.wave,
            "base_hz" => &mut self.base_hz,
            "mod_hz" => &mut self.mod_hz,
            "mod_idx" => &mut self.mod_idx,
            _ => panic!("Modulator only does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for Modulator {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        assert!(n < graph.nodes.len());
        if let Some(v) = graph.nodes[n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn set_mod_hz(graph: &Graph, n: Tag, hz: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Modulator>()
    {
        v.mod_hz = fix(hz);
    }
}

pub fn set_mod_idx(graph: &Graph, n: Tag, idx: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Modulator>()
    {
        v.mod_idx = fix(idx);
    }
}

pub fn set_base_hz(graph: &Graph, n: Tag, hz: Real) {
    assert!(n < graph.nodes.len());
    if let Some(v) = graph.nodes[n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Modulator>()
    {
        v.base_hz = fix(hz);
    }
}
