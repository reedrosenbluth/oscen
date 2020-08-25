use crate::osc::{ConstBuilder, OscBuilder};
use crate::rack::*;
use crate::{build, props, tag};
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
    pub fn rack(&self, rack: &mut Rack) -> Box<Mixer> {
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
    fn signal(
        &mut self,
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
        let out = self.waves.iter().fold(0.0, |acc, n| acc + outputs[(*n, 0)]);
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
    build!(active);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<Union> {
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
    fn signal(
        &mut self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
        let idx = self.active(controls, outputs);
        let wave = self.waves[idx];
        outputs[(self.tag, 0)] = outputs[(wave, 0)];
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
    pub fn rack(&self, rack: &mut Rack) -> Box<Product> {
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
    fn signal(
        &mut self,
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
        let out = self.waves.iter().fold(1.0, |acc, n| acc * outputs[(*n, 0)]);
        outputs[(self.tag, 0)] = out;
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Vca {
    tag: Tag,
    wave: Tag,
}

impl Vca {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
    props!(level, set_level, 0);
}

impl Signal for Vca {
    tag!();
    fn signal(
        &mut self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
        outputs[(self.tag, 0)] = self.level(controls, outputs) * outputs[(self.wave, 0)];
    }
}

#[derive(Copy, Clone)]
pub struct VcaBuilder {
    wave: Tag,
    level: Control,
}

impl VcaBuilder {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            level: 1.into(),
        }
    }
    build!(level);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<Vca> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.level;
        let vca = Box::new(Vca::new(tag, self.wave));
        rack.push(vca.clone());
        vca
    }
}

#[derive(Debug, Copy, Clone)]
pub struct CrossFade {
    tag: Tag,
    wave1: Tag,
    wave2: Tag,
}

impl CrossFade {
    pub fn new(tag: Tag, wave1: Tag, wave2: Tag) -> Self {
        Self { tag, wave1, wave2 }
    }
    props!(alpha, set_alpha, 0);
}

impl Signal for CrossFade {
    tag!();
    fn signal(
        &mut self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
        let alpha = self.alpha(controls, outputs);
        outputs[(self.tag, 0)] =
            alpha * outputs[(self.wave2, 0)] + (1.0 - alpha) * outputs[(self.wave1, 0)];
    }
}

#[derive(Debug, Copy, Clone)]
pub struct CrossFadeBuilder {
    wave1: Tag,
    wave2: Tag,
    alpha: Control,
}

impl CrossFadeBuilder {
    pub fn new(wave1: Tag, wave2: Tag) -> Self {
        Self {
            wave1,
            wave2,
            alpha: 0.5.into(),
        }
    }
    build!(alpha);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<CrossFade> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.alpha;
        let cf = Box::new(CrossFade::new(tag, self.wave1, self.wave2));
        rack.push(cf.clone());
        cf
    }
}

#[derive(Clone)]
pub struct Modulator {
    tag: Tag,
    hz_tag: Tag,
    ratio_tag: Tag,
    index_tag: Tag,
}

impl Modulator {
    pub fn new(tag: Tag, hz_tag: Tag, ratio_tag: Tag, index_tag: Tag) -> Self {
        Self {
            tag,
            hz_tag,
            ratio_tag,
            index_tag,
        }
    }
    pub fn hz(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls[(self.hz_tag, 0)];
        outputs.value(inp).unwrap()
    }
    pub fn set_hz(&self, controls: &mut Controls, value: Control) {
        controls[(self.hz_tag, 0)] = value;
    }
    pub fn ratio(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls[(self.ratio_tag, 0)];
        outputs.value(inp).unwrap()
    }
    pub fn set_ratio(&self, controls: &mut Controls, value: Control) {
        controls[(self.ratio_tag, 0)] = value;
    }
    pub fn index(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls[(self.index_tag, 0)];
        outputs.value(inp).unwrap()
    }
    pub fn set_index(&self, controls: &mut Controls, value: Control) {
        controls[(self.index_tag, 0)] = value;
    }
}

impl Signal for Modulator {
    tag!();
    fn signal(
        &mut self,
        _controls: &Controls,
        _state: &mut State,
        _outputs: &mut Outputs,
        _sample_rate: Real,
    ) {
    }
}

#[derive(Debug, Clone)]
pub struct ModulatorBuilder {
    hz: Control,
    ratio: Control,
    index: Control,
    signal_fn: SignalFn,
}

impl ModulatorBuilder {
    pub fn new(signal_fn: SignalFn) -> Self {
        Self {
            hz: 0.into(),
            ratio: 1.into(),
            index: 0.into(),
            signal_fn,
        }
    }
    build!(hz);
    build!(ratio);
    build!(index);
    pub fn rack(
        &self,
        rack: &mut Rack,
        controls: &mut Controls,
        state: &mut State,
    ) -> Box<Modulator> {
        let hz = ConstBuilder::new(self.hz).rack(rack, controls);
        let ratio = ConstBuilder::new(self.ratio).rack(rack, controls);
        let index = ConstBuilder::new(self.index).rack(rack, controls);
        let mod_hz = ProductBuilder::new(vec![hz.tag(), ratio.tag()]).rack(rack);
        let mod_amp = ProductBuilder::new(vec![hz.tag(), ratio.tag(), index.tag()]).rack(rack);
        // let mod_amp = MixerBuilder::new(vec![hz.tag(), amp_factor.tag()]).rack(rack);
        let modulator = OscBuilder::new(self.signal_fn)
            .amplitude(mod_amp.cv())
            .hz(mod_hz.cv())
            .rack(rack, controls, state);
        let carrier_hz = MixerBuilder::new(vec![modulator.tag(), hz.tag()]).rack(rack);
        Box::new(Modulator::new(
            carrier_hz.tag(),
            hz.tag(),
            ratio.tag(),
            index.tag(),
        ))
    }
}