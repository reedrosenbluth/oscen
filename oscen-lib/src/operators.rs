use crate::oscillators::{ConstBuilder, OscBuilder};
use crate::rack::*;
use crate::utils::{arc_mutex, ArcMutex};
use crate::{build, props, tag};
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Mixer> {
        let n = rack.num_modules();
        let cs = rack.controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let mix = arc_mutex(Mixer::new(n.into(), nw));
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let cs = &rack.controls.controls(self.tag())[0..self.num_waves as usize];
        rack.outputs[(self.tag, 0)] = cs
            .iter()
            .map(|x| x.idx())
            .fold(0.0, |acc, n| acc + rack.outputs[(n, 0)]);
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Union> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.active;
        let cs = rack.controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i + 1] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let u = arc_mutex(Union::new(n.into(), nw));
        rack.push(u.clone());
        u
    }
}

impl Union {
    pub fn new(tag: Tag, num_waves: u8) -> Self {
        Self { tag, num_waves }
    }
    pub fn active(&self, rack: &Rack) -> usize {
        let inp = rack.controls[(self.tag, 0)];
        rack.outputs
            .integer(inp)
            .expect("active must be Control::I")
    }
    pub fn set_active(&self, rack: &mut Rack, value: Control) {
        rack.controls[(self.tag, 0)] = value;
    }
}

impl Signal for Union {
    tag!();
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let idx = self.active(rack);
        let cs = &rack.controls.controls(self.tag())[1..=self.num_waves as usize];
        let c: Tag = cs[idx].idx().into();
        rack.outputs[(self.tag, 0)] = rack.outputs[(c, 0)];
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Product> {
        let n = rack.num_modules();
        let cs = rack.controls.controls_mut(n);
        for (i, w) in self.waves.iter().enumerate() {
            cs[i] = Control::I((*w).into());
        }
        let nw = self.waves.len() as u8;
        let p = arc_mutex(Product::new(n.into(), nw));
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let cs = &rack.controls.controls(self.tag())[0..self.num_waves as usize];
        rack.outputs[(self.tag, 0)] = cs
            .iter()
            .map(|x| x.idx())
            .fold(1.0, |acc, n| acc * rack.outputs[(n, 0)]);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Inverse {
    tag: Tag,
    wave: Tag,
}

impl Inverse {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
}

impl Signal for Inverse {
    tag!();

    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = 1.0 / rack.outputs[(self.wave, 0)];
    }
}

#[derive(Copy, Clone)]
pub struct InverseBuilder {
    wave: Tag,
}

impl InverseBuilder {
    pub fn new(wave: Tag) -> Self {
        Self { wave }
    }

    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Inverse> {
        let n = rack.num_modules();
        let inverse = arc_mutex(Inverse::new(n.into(), self.wave));
        rack.push(inverse.clone());
        inverse
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = self.level(rack) * rack.outputs[(self.wave, 0)];
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Vca> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.level;
        let vca = arc_mutex(Vca::new(n.into(), self.wave));
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let alpha = self.alpha(rack);
        rack.outputs[(self.tag, 0)] =
            alpha * rack.outputs[(self.wave2, 0)] + (1.0 - alpha) * rack.outputs[(self.wave1, 0)];
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<CrossFade> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.alpha;
        let cf = arc_mutex(CrossFade::new(n.into(), self.wave1, self.wave2));
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
    pub fn hz(&self, rack: &Rack) -> f32 {
        let inp = rack.controls[(self.hz_tag, 0)];
        rack.outputs.value(inp).unwrap()
    }
    pub fn set_hz(&self, rack: &mut Rack, value: Control) {
        rack.controls[(self.hz_tag, 0)] = value;
    }
    pub fn ratio(&self, rack: &Rack) -> f32 {
        let inp = rack.controls[(self.ratio_tag, 0)];
        rack.outputs.value(inp).unwrap()
    }
    pub fn set_ratio(&self, rack: &mut Rack, value: Control) {
        rack.controls[(self.ratio_tag, 0)] = value;
    }
    pub fn index(&self, rack: &Rack) -> f32 {
        let inp = rack.controls[(self.index_tag, 0)];
        rack.outputs.value(inp).unwrap()
    }
    pub fn set_index(&self, rack: &mut Rack, value: Control) {
        rack.controls[(self.index_tag, 0)] = value;
    }
}

impl Signal for Modulator {
    tag!();
    fn signal(&mut self, _rack: &mut Rack, _sample_rate: f32) {}
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Modulator> {
        let hz = ConstBuilder::new(self.hz).rack(rack);
        let ratio = ConstBuilder::new(self.ratio).rack(rack);
        let index = ConstBuilder::new(self.index).rack(rack);
        let mod_hz = ProductBuilder::new(vec![hz.lock().tag(), ratio.lock().tag()]).rack(rack);
        let mod_amp = ProductBuilder::new(vec![
            hz.lock().tag(),
            ratio.lock().tag(),
            index.lock().tag(),
        ])
        .rack(rack);
        let modulator = OscBuilder::new(self.signal_fn)
            .amplitude(mod_amp.lock().tag())
            .hz(mod_hz.lock().tag())
            .rack(rack);
        let carrier_hz =
            MixerBuilder::new(vec![modulator.lock().tag(), hz.lock().tag()]).rack(rack);
        let wrapped_modulator = arc_mutex(Modulator::new(
            carrier_hz.lock().tag(),
            hz.lock().tag(),
            ratio.lock().tag(),
            index.lock().tag(),
        ));
        wrapped_modulator
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
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let val = rack.outputs[(self.wave, 0)];
        let d = self.delay(rack) * sample_rate;
        rack.buffers.buffers_mut(self.tag).push(val);
        rack.outputs[(self.tag, 0)] = rack.buffers.buffers(self.tag).get_cubic(d);
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

    build!(delay);

    pub fn rack(&mut self, rack: &mut Rack) -> ArcMutex<Delay> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.delay;
        let delay = arc_mutex(Delay::new(n, self.wave));
        rack.buffers
            .set_buffer(delay.lock().tag, RingBuffer::new32(44100.0));
        rack.push(delay.clone());
        delay
    }
}
