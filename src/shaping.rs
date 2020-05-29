use super::graph::*;
use std::any::Any;

pub struct SineFold {
    pub tag: Tag,
    pub wave: Tag,
}

impl SineFold {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }

    pub fn wrapped(tag: Tag, wave: Tag) -> ArcMutex<Self> {
        arc(Self::new(tag, wave))
    }
}

impl Signal for SineFold {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let a = graph.output(&self.wave);
        (a * TAU * 1.0 / 2.5).sin()
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}
