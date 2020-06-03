use super::graph::*;
use std::any::Any;
use std::ops::{Index, IndexMut};

#[derive(Clone)]
pub struct Union {
    pub tag: Tag,
    pub waves: Vec<Tag>,
    pub active: Tag,
    pub level: In,
}

impl Union {
    pub fn new(waves: Vec<Tag>) -> Self {
        let active = waves[0];
        Union {
            tag: mk_tag(),
            waves,
            active,
            level: (1.0).into(),
        }
    }

    pub fn wrapped(waves: Vec<Tag>) -> ArcMutex<Self> {
        arc(Union::new(waves))
    }
}

impl Signal for Union {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        In::val(graph, self.level) * graph.output(self.active)
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<usize> for Union {
    type Output = Tag;

    fn index(&self, index: usize) -> &Self::Output {
        &self.waves[index]
    }
}

impl IndexMut<usize> for Union {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.waves[index]
    }
}
#[derive(Clone)]
pub struct Product {
    pub tag: Tag,
    pub waves: Vec<Tag>,
}

impl Product {
    pub fn new(waves: Vec<Tag>) -> Self {
        Product {
            tag: mk_tag(),
            waves,
        }
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
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<usize> for Product {
    type Output = Tag;

    fn index(&self, index: usize) -> &Self::Output {
        &self.waves[index]
    }
}

impl IndexMut<usize> for Product {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.waves[index]
    }
}

#[derive(Clone)]
pub struct Vca {
    pub tag: Tag,
    pub wave: Tag,
    pub level: In,
}

impl Vca {
    pub fn new(wave: Tag, level: In) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            level: level,
        }
    }

    pub fn wrapped(wave: Tag, level: In) -> ArcMutex<Self> {
        arc(Self::new(wave, level))
    }
}

impl Signal for Vca {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        graph.output(self.wave) * In::val(graph, self.level)
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for Vca {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "level" => &self.level,
            _ => panic!("Vca does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Vca {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "level" => &mut self.level,
            _ => panic!("Vca does not have a field named: {}", index),
        }
    }
}

#[derive(Clone)]
pub struct Mixer {
    pub tag: Tag,
    pub waves: Vec<Tag>,
    pub levels: Vec<In>,
    pub level: In,
}

impl Mixer {
    pub fn new(waves: Vec<Tag>) -> Self {
        let levels = waves.iter().map(|_| (1.0).into()).collect();
        Mixer {
            tag: mk_tag(),
            waves,
            levels,
            level: (1.0).into(),
        }
    }

    pub fn wrapped(waves: Vec<Tag>) -> ArcMutex<Self> {
        arc(Mixer::new(waves))
    }
}

impl Signal for Mixer {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        self.waves.iter().enumerate().fold(0.0, |acc, (i, n)| {
            acc + graph.output(*n) * In::val(graph, self.levels[i])
        }) * In::val(graph, self.level)
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<usize> for Mixer {
    type Output = Tag;

    fn index(&self, index: usize) -> &Self::Output {
        &self.waves[index]
    }
}

impl IndexMut<usize> for Mixer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.waves[index]
    }
}
#[derive(Clone)]
pub struct Lerp {
    pub tag: Tag,
    pub wave1: In,
    pub wave2: In,
    pub alpha: In,
}

impl Lerp {
    pub fn new(wave1: Tag, wave2: Tag) -> Self {
        Lerp {
            tag: mk_tag(),
            wave1: wave1.into(),
            wave2: wave2.into(),
            alpha: (0.5).into(),
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
        alpha * In::val(graph, self.wave2) + (1.0 - alpha) * In::val(graph, self.wave1)
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for Lerp {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "wave1" => &self.wave1,
            "wave2" => &self.wave2,
            "alpha" => &self.alpha,
            _ => panic!("Lerp does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Lerp {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "wave1" => &mut self.wave1,
            "wave2" => &mut self.wave2,
            "alpha" => &mut self.alpha,
            _ => panic!("Lerp does not have a field named: {}", index),
        }
    }
}

impl<'a> Set<'a> for Lerp {
    fn set(graph: &mut Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.get_node(n)
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

pub fn set_alpha(graph: &Graph, k: In, a: Real) {
    match k {
        In::Cv(n) => {
            if let Some(v) = graph.nodes[&n]
                .module
                .lock()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<Lerp>()
            {
                v.alpha = a.into()
            }
        }
        In::Fix(_) => panic!("Lerp wave can only be a In::Var"),
    }
}

pub struct Lerp3 {
    pub tag: Tag,
    pub lerp1: In,
    pub lerp2: In,
    pub knob: In,
}

impl Lerp3 {
    pub fn new(lerp1: Tag, lerp2: Tag, knob: In) -> Self {
        Self {
            tag: mk_tag(),
            lerp1: lerp1.into(),
            lerp2: lerp2.into(),
            knob,
        }
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
            In::val(graph, self.lerp1)
        } else {
            In::val(graph, self.lerp2)
        }
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for Lerp3 {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "lerp1" => &self.lerp1,
            "lerp2" => &self.lerp2,
            "knob" => &self.knob,
            _ => panic!("Lerp3 does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Lerp3 {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "lerp1" => &mut self.lerp1,
            "lerp2" => &mut self.lerp2,
            "knob" => &mut self.knob,
            _ => panic!("Lerp does not have a field named: {}", index),
        }
    }
}

impl<'a> Set<'a> for Lerp3 {
    fn set(graph: &mut Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.get_node(n)
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

pub fn set_knob(graph: &Graph, n: Tag, k: Real) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Lerp3>()
    {
        v.knob = k.into();
        v.set_alphas(graph);
    }
}

pub struct Modulator {
    pub tag: Tag,
    pub wave: In,
    pub base_hz: In,
    pub mod_hz: In,
    pub mod_idx: In,
}

impl Modulator {
    pub fn new(wave: Tag, base_hz: In, mod_hz: In, mod_idx: In) -> Self {
        Modulator {
            tag: mk_tag(),
            wave: wave.into(),
            base_hz,
            mod_hz,
            mod_idx,
        }
    }

    pub fn wrapped(wave: Tag, base_hz: In, mod_hz: In, mod_idx: In) -> ArcMutex<Self> {
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
        base_hz + mod_idx * mod_hz * In::val(graph, self.wave)
    }
    fn tag(&self) -> Tag {
        self.tag
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
    fn set(graph: &mut Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.get_node(n)
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}
