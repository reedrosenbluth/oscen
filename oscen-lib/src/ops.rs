use crate::osc::{ConstBuilder, OscBuilder};
use crate::rack::*;
use crate::uti::RingBuffer;
use crate::{build, props, tag};
use std::sync::Arc;
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
    pub fn rack(&self, rack: &mut Rack) -> Arc<Mixer> {
        let n = rack.num_modules();
        let mix = Arc::new(Mixer::new(n.into(), self.waves.clone()));
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
        &self,
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: f32,
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Union> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.active;
        let u = Arc::new(Union::new(n.into(), self.waves.clone()));
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
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: f32,
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
    pub fn rack(&self, rack: &mut Rack) -> Arc<Product> {
        let n = rack.num_modules();
        let p = Arc::new(Product::new(n.into(), self.waves.clone()));
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
        &self,
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _sample_rate: f32,
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
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
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
            level: 1.into(),
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
    ) -> Arc<Modulator> {
        let hz = ConstBuilder::new(self.hz).rack(rack, controls);
        let ratio = ConstBuilder::new(self.ratio).rack(rack, controls);
        let index = ConstBuilder::new(self.index).rack(rack, controls);
        let mod_hz = ProductBuilder::new(vec![hz.tag(), ratio.tag()]).rack(rack);
        let mod_amp = ProductBuilder::new(vec![hz.tag(), ratio.tag(), index.tag()]).rack(rack);
        // let mod_amp = MixerBuilder::new(vec![hz.tag(), amp_factor.tag()]).rack(rack);
        let modulator = OscBuilder::new(self.signal_fn)
            .amplitude(mod_amp.tag())
            .hz(mod_hz.tag())
            .rack(rack, controls, state);
        let carrier_hz = MixerBuilder::new(vec![modulator.tag(), hz.tag()]).rack(rack);
        Arc::new(Modulator::new(
            carrier_hz.tag(),
            hz.tag(),
            ratio.tag(),
            index.tag(),
        ))
    }
}

/*
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
    props!(delay_time, set_delay_time, 0);
}

impl Signal for Delay {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        sample_rate: f32,
    ) {
        let delay = self.delay_time(controls, outputs) * sample_rate;
        let rp = self.ring_buffer.read_pos;
        let wp = (delay + rp).ceil();
        self.ring_buffer.set_write_pos(wp as usize);
        self.ring_buffer.set_read_pos(rp - delay);
        if delay > self.ring_buffer.len() as f32 - 3.0 {
            panic!("Ring buffer too small for dalay {}", delay);
        }
        let val = outputs[(self.wave, 0)];
        self.ring_buffer.push(val);
        outputs[(self.tag, 0)] = self.ring_buffer.get_cubic();
    }
}

pub struct DelayBuilder<'a> {
    wave: Tag,
    ring_buffer: &'a mut RingBuffer<'a, f32>,
    delay_time: Control,
}

impl<'a> DelayBuilder<'a> {
    pub fn new(wave: Tag, ring_buffer: &'a mut RingBuffer<'a, f32>, delay_time: Control) -> Self {
        Self {
            wave,
            ring_buffer,
            delay_time,
        }
    }
    build!(delay_time);
    pub fn rack(&'static mut self, rack: &mut Rack, controls: &mut Controls) -> Arc<Delay> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.delay_time;
        let wave = self.wave;
        // let mut ring_buffer = &*self.ring_buffer;
        let ring_buffer = &mut *self.ring_buffer;
        let delay = Arc::new(Delay::new(n, wave));
        rack.push(delay);
        delay
    }
}
*/