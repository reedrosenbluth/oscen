use super::graph::*;
use std::any::Any;

pub struct SineFold {
    pub wave: Tag,
}

impl SineFold {
    pub fn new(wave: Tag) -> Self {
        Self { wave }
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
        (a * TAU * 1.0 / 2.5).sin()
    }
}
