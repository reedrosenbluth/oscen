use super::signal::*;
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

    pub fn fold_param(&mut self, arg: In) -> &mut Self {
        self.fold_param = arg;
        self
    }
}

impl Builder for SineFold {}

impl Signal for SineFold {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let a = rack.output(self.wave);
        let fold_param = In::val(rack, self.fold_param);
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
}

impl Builder for Tanh {}

impl Signal for Tanh {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let a = rack.output(self.wave);
        (a * TAU).tanh()
    }
}