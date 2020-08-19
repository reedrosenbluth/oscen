use crate::rack::*;
use crate::tag;
#[derive(Debug, Clone)]
pub struct Mixer {
    tag: Tag,
    waves: Vec<Tag>,
}

#[derive(Debug, Clone)]
pub struct MixerBuilder {
    waves: Vec<Tag>,
}

impl MixerBuilder {
    pub fn new(waves: Vec<Tag>) -> Self {
        Self { waves }
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack) -> Box<Mixer> {
        let tag = rack.num_modules();
        let mix = Box::new(Mixer::new(tag, self.waves.clone()));
        rack.push(mix.clone());
        mix
    }
}

impl Mixer {
    fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
}

impl Signal for Mixer {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let out = self
            .waves
            .iter()
            .fold(0.0, |acc, n| acc + outputs.outputs(*n)[0]);
        outputs[(self.tag, 0)] = out;
    }
}

#[derive(Debug, Clone)]
pub struct Union {
    tag: Tag,
    waves: Vec<Tag>,
}

#[derive(Clone)]
pub struct UnionBuilder {
    waves: Vec<Tag>,
    active: Control,
}

impl UnionBuilder {
    pub fn new(waves: Vec<Tag>) -> Self {
        Self {
            waves,
            active: Control::I(0),
        }
    }
    pub fn active(&mut self, value: usize) -> &mut Self {
        self.active = Control::I(value);
        self
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack, controls: &mut Controls) -> Box<Union> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.active;
        let u = Box::new(Union::new(tag, self.waves.clone()));
        rack.push(u.clone());
        u
    }
}

impl Union {
    pub fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
    pub fn active(&self, controls: &Controls, outputs: &Outputs) -> usize {
        let inp = controls[(self.tag, 0)];
        outputs.integer(inp).expect("active must be Control::I")
    }
    pub fn set_active(&self, controls: &mut Controls, value: Control) {
        controls[(self.tag, 0)] = value;
    }
}

impl Signal for Union {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let idx = self.active(controls, outputs);
        let wave = self.waves[idx];
        outputs[(self.tag, 0)] = outputs.outputs(wave)[0];
    }
}

#[derive(Debug, Clone)]
pub struct Product {
    tag: Tag,
    waves: Vec<Tag>,
}

#[derive(Debug, Clone)]
pub struct ProductBuilder {
    waves: Vec<Tag>,
}

impl ProductBuilder {
    pub fn new(waves: Vec<Tag>) -> Self {
        Self { waves }
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack) -> Box<Product> {
        let tag = rack.num_modules();
        let p = Box::new(Product::new(tag, self.waves.clone()));
        rack.push(p.clone());
        p
    }
}

impl Product {
    fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
}

impl Signal for Product {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let out = self
            .waves
            .iter()
            .fold(1.0, |acc, n| acc * outputs.outputs(*n)[0]);
        outputs[(self.tag, 0)] = out;
    }
}
