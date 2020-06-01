use super::graph::*;
use std::any::Any;

pub struct SineFold {
    pub tag: Tag,
    pub wave: Tag,
    pub fold_param: In,
}

impl SineFold {
    pub fn new(wave: Tag) -> Self {
        Self { tag: mk_tag(), wave, fold_param: fix(TAU) }
    }

    pub fn wrapped(wave: Tag) -> ArcMutex<Self> {
        arc(Self::new(wave))
    }
}

impl Signal for SineFold {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let a = graph.output(self.wave);
        let fold_param = In::val(graph, self.fold_param);
        (a * TAU / fold_param).sin()
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

pub struct Tanh {
    pub tag: Tag,
    pub wave: Tag,
}

impl Tanh {
    pub fn new(wave: Tag) -> Self {
        Self { tag: mk_tag(), wave}
    }    

    pub fn wrapped(wave: Tag) ->ArcMutex<Self> {
        arc(Self::new(wave))
    }
}

impl Signal for Tanh {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let a = graph.output(self.wave);
        (a * TAU).tanh()
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}