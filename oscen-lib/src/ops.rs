use crate::rack::*;
use crate::tag;
#[derive(Debug, Clone)]
pub struct Mixer {
    tag: Tag,
    waves: Vec<Tag>,
}

impl Mixer {
    pub fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
    pub fn rack<'a>(
        rack: &'a mut Rack,
        waves: Vec<Tag>,
    ) -> Box<Self> {
        let tag = rack.num_modules();
        let mix = Box::new(Self::new(tag, waves));
        rack.push(mix.clone());
        mix
    }
}

impl Signal for Mixer {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let out = self
            .waves
            .iter()
            .fold(0.0, |acc, n| acc + outputs.outputs(*n)[0]);
        outputs.outputs_mut(self.tag)[0] = out;
    }
}