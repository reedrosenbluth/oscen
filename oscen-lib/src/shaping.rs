use crate::rack::*;
use crate::{props, tag};
use std::f32::consts::PI;
use std::sync::Arc;

#[derive(Debug, Copy, Clone)]
pub struct SineFold {
    tag: Tag,
    wave: Tag,
    // fold_param: In,
}

impl SineFold {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }

    props!(fold_param, set_fold_param, 0);
}

impl Signal for SineFold {
    tag!();

    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        let fold_param = self.fold_param(rack);
        rack.outputs[(self.tag, 0)] = (rack.outputs[(self.wave, 0)] * 2.0 * PI / fold_param).sin();
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SineFoldBuilder {
    wave: Tag,
    fold_param: Control,
}

impl SineFoldBuilder {
    pub fn new(wave: Tag, fold_param: Control) -> Self {
        Self { wave, fold_param }
    }

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<SineFold> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.fold_param;
        let sf = Arc::new(SineFold::new(n.into(), self.wave));
        rack.push(sf.clone());
        sf
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Tanh {
    tag: Tag,
    wave: Tag,
}

impl Tanh {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
}

impl Signal for Tanh {
    tag!();

    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = (rack.outputs[(self.wave, 0)] * 2.0 * PI).tanh();
    }
}

#[derive(Debug, Copy, Clone)]
pub struct TanhBuilder {
    wave: Tag,
}

impl TanhBuilder {
    pub fn new(wave: Tag) -> Self {
        Self { wave }
    }

    pub fn rack(&self, rack: &mut Rack) -> Arc<Tanh> {
        let n = rack.num_modules();
        let t = Arc::new(Tanh::new(n.into(), self.wave));
        rack.push(t.clone());
        t
    }
}
