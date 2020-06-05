use super::graph::*;
use crate::{std_signal, as_any_mut};
use std::any::Any;

pub struct SineFold {
    pub tag: Tag,
    pub wave: Tag,
    pub fold_param: In,
}

impl SineFold {
    pub fn new(wave: Tag) -> Self {
        Self { tag: mk_tag(), wave, fold_param: TAU.into() }
    }

    pub fn wrapped(wave: Tag) -> ArcMutex<Self> {
        arc(Self::new(wave))
    }
}

impl Signal for SineFold {
    std_signal!();
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let a = graph.output(self.wave);
        let fold_param = In::val(graph, self.fold_param);
        (a * TAU / fold_param).sin()
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
    std_signal!();
    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let a = graph.output(self.wave);
        (a * TAU).tanh()
    }
}