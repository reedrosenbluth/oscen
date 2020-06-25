use super::signal::*;
use crate::{as_any_mut, std_signal};
use std::any::Any;

pub struct SineFold {
    tag: Tag,
    wave: Tag,
    fold_param: In,
}

impl SineFold {
    pub fn new(wave: Tag) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            fold_param: TAU.into(),
        }
    }

    pub fn wave(&mut self, arg: Tag) -> &mut Self {
        self.wave = arg;
        self
    }

    pub fn fold_param<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.fold_param = arg.into();
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
    tag: Tag,
    wave: Tag,
}

impl Tanh {
    pub fn new(wave: Tag) -> Self {
        Self {
            tag: mk_tag(),
            wave,
        }
    }

    pub fn wave(&mut self, arg: Tag) -> &mut Self {
        self.wave = arg;
        self
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
