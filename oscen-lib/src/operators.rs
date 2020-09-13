use crate::oscillators::{ConstBuilder, OscBuilder};
use crate::rack::*;
use crate::{build, props, tag};
use std::sync::Arc;
#[derive(Debug, Clone)]
pub struct Mixer {
    tag: Tag,
    num_waves: u8,
}

#[derive(Debug, Clone)]
pub struct MixerBuilder {
    waves: Vec<Tag>,
}

impl MixerBuilder {
    pub fn new(waves: Vec<Tag>) -> Self {
        Self { waves }
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Mixer> {
        let n = rack.num_modules();
        let cs = controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let mix = Arc::new(Mixer::new(n.into(), nw));
        rack.push(mix.clone());
        mix
    }
}

impl Mixer {
    fn new(tag: Tag, num_waves: u8) -> Self {
        Self { tag, num_waves }
    }
}

impl Signal for Mixer {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let cs = &controls.controls(self.tag())[0..self.num_waves as usize];
        outputs[(self.tag, 0)] = cs
            .iter()
            .map(|x| x.idx())
            .fold(0.0, |acc, n| acc + outputs[(n, 0)]);
    }
}

#[derive(Debug, Clone)]
pub struct Union {
    tag: Tag,
    num_waves: u8,
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Union> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.active;
        let cs = controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i + 1] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let u = Arc::new(Union::new(n.into(), nw));
        rack.push(u.clone());
        u
    }
}

impl Union {
    pub fn new(tag: Tag, num_waves: u8) -> Self {
        Self { tag, num_waves }
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
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let idx = self.active(controls, outputs);
        let cs = &controls.controls(self.tag())[1..=self.num_waves as usize];
        let c: Tag = cs[idx].idx().into();
        outputs[(self.tag, 0)] = outputs[(c, 0)];
    }
}

#[derive(Debug, Clone)]
pub struct Product {
    tag: Tag,
    num_waves: u8,
}

#[derive(Debug, Clone)]
pub struct ProductBuilder {
    waves: Vec<Tag>,
}

impl ProductBuilder {
    pub fn new(waves: Vec<Tag>) -> Self {
        Self { waves }
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Product> {
        let n = rack.num_modules();
        let cs = controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let p = Arc::new(Product::new(n.into(), nw));
        rack.push(p.clone());
        p
    }
}

impl Product {
    fn new(tag: Tag, num_waves: u8) -> Self {
        Self { tag, num_waves }
    }
}

impl Signal for Product {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let cs = &controls.controls(self.tag())[0..self.num_waves as usize];
        outputs[(self.tag, 0)] = cs
            .iter()
            .map(|x| x.idx())
            .fold(1.0, |acc, n| acc + outputs[(n, 0)]);
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
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
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
            level: 1.0.into(),
        }
    }
    build!(level);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Vca> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.level;
        let vca = Arc::new(Vca::new(n.into(), self.wave));
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
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<CrossFade> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.alpha;
        let cf = Arc::new(CrossFade::new(n.into(), self.wave1, self.wave2));
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
    pub fn hz(&self, controls: &Controls, outputs: &Outputs) -> f32 {
        let inp = controls[(self.hz_tag, 0)];
        outputs.value(inp).unwrap()
    }
    pub fn set_hz(&self, controls: &mut Controls, value: Control) {
        controls[(self.hz_tag, 0)] = value;
    }
    pub fn ratio(&self, controls: &Controls, outputs: &Outputs) -> f32 {
        let inp = controls[(self.ratio_tag, 0)];
        outputs.value(inp).unwrap()
    }
    pub fn set_ratio(&self, controls: &mut Controls, value: Control) {
        controls[(self.ratio_tag, 0)] = value;
    }
    pub fn index(&self, controls: &Controls, outputs: &Outputs) -> f32 {
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
        &self,
        _controls: &Controls,
        _state: &mut State,
        _outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
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
            hz: 0.0.into(),
            ratio: 1.0.into(),
            index: 0.0.into(),
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
    ) -> Arc<Modulator> {
        let hz = ConstBuilder::new(self.hz).rack(rack, controls);
        let ratio = ConstBuilder::new(self.ratio).rack(rack, controls);
        let index = ConstBuilder::new(self.index).rack(rack, controls);
        let mod_hz = ProductBuilder::new(vec![hz.tag(), ratio.tag()]).rack(rack, controls);
        let mod_amp =
            ProductBuilder::new(vec![hz.tag(), ratio.tag(), index.tag()]).rack(rack, controls);
        let modulator = OscBuilder::new(self.signal_fn)
            .amplitude(mod_amp.tag())
            .hz(mod_hz.tag())
            .rack(rack, controls, state);
        let carrier_hz = MixerBuilder::new(vec![modulator.tag(), hz.tag()]).rack(rack, controls);
        Arc::new(Modulator::new(
            carrier_hz.tag(),
            hz.tag(),
            ratio.tag(),
            index.tag(),
        ))
    }
}

pub struct Delay {
    tag: Tag,
    wave: Tag,
}

impl Delay {
    pub fn new<T: Into<Tag>>(tag: T, wave: Tag) -> Self {
        Delay {
            tag: tag.into(),
            wave,
        }
    }
    props!(delay, set_delay, 0);
}

impl Signal for Delay {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let val = outputs[(self.wave, 0)];
        buffers
            .buffers_mut(self.tag)
            .delay(self.delay(controls, outputs), sample_rate);
        buffers.buffers_mut(self.tag).push(val);
        outputs[(self.tag, 0)] = buffers.buffers(self.tag).get_cubic();
    }
}

pub struct DelayBuilder {
    wave: Tag,
    delay: Control,
}

impl DelayBuilder {
    pub fn new(wave: Tag, delay: Control) -> Self {
        Self { wave, delay }
    }
    pub fn rack(&mut self, rack: &mut Rack, buffers: &mut Buffers) -> Arc<Delay> {
        let n = rack.num_modules();
        let delay = Arc::new(Delay::new(n, self.wave));
        // buffers.set_buffer(delay.tag(), RingBuffer::new32(self.delay(), 44100.0));
        rack.push(delay.clone());
        delay
    }
}
