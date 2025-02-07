use crate::rack::*;
use crate::utils::{arc_mutex, ArcMutex};
use crate::{build, props, tag};
use std::f32::consts::PI;

#[derive(Debug, Copy, Clone)]
pub struct Lpf {
    tag: Tag,
    wave: Tag,
    s0: f32,
    s1: f32,
    s2: f32,
    s3: f32,
}

impl Lpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            s0: 0.0,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
        }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, rack: &Rack) -> bool {
        let ctrl = rack.controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {ctrl:?}"),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Lpf {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let x0 = rack.outputs[(self.wave, 0)];
        let cut_off = self.cutoff(rack);
        if self.off(rack) || cut_off > 20_000.0 {
            rack.outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(rack);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 + b1 + b2);
        let a1 = 2.0 * a0;
        rack.outputs[(tag, 0)] =
            a0 * x0 + a1 * self.s0 + a0 * self.s1 - b1 * self.s2 - b2 * self.s3;
        self.s1 = self.s0;
        self.s0 = x0;
        self.s3 = self.s2;
        self.s2 = if rack.outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            rack.outputs[(tag, 0)]
        };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct LpfBuilder {
    wave: Tag,
    cut_off: Control,
    q: Control,
    off: Control,
}

impl LpfBuilder {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            cut_off: 25_000.0.into(),
            q: 0.707.into(),
            off: false.into(),
        }
    }

    build!(cut_off);
    build!(q);
    build!(off);

    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Lpf> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.cut_off;
        rack.controls[(n, 1)] = self.q;
        rack.controls[(n, 2)] = self.off;
        let lpf = arc_mutex(Lpf::new(n.into(), self.wave));
        rack.push(lpf.clone());
        lpf
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Hpf {
    tag: Tag,
    wave: Tag,
    s0: f32,
    s1: f32,
    s2: f32,
    s3: f32,
}

impl Hpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            s0: 0.0,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
        }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, rack: &Rack) -> bool {
        let ctrl = rack.controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {ctrl:?}"),
        }
    }
    pub fn set_off(&self, rack: &mut Rack, value: bool) {
        rack.controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Hpf {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let x0 = rack.outputs[(self.wave, 0)];
        let cut_off = self.cutoff(rack);
        if self.off(rack) || cut_off > 20_000.0 {
            rack.outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(rack);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 - b1 + b2);
        let a1 = -2.0 * a0;
        rack.outputs[(tag, 0)] =
            a0 * x0 + a1 * self.s0 + a0 * self.s1 - b1 * self.s2 - b2 * self.s3;
        self.s1 = self.s0;
        self.s0 = x0;
        self.s3 = self.s2;
        self.s2 = if rack.outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            rack.outputs[(tag, 0)]
        };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct HpfBuilder {
    wave: Tag,
    cut_off: Control,
    q: Control,
    off: Control,
}

impl HpfBuilder {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            cut_off: 25_000.0.into(),
            q: 0.707.into(),
            off: false.into(),
        }
    }

    build!(cut_off);
    build!(q);
    build!(off);

    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Hpf> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.cut_off;
        rack.controls[(n, 1)] = self.q;
        rack.controls[(n, 2)] = self.off;
        let hpf = arc_mutex(Hpf::new(n.into(), self.wave));
        rack.push(hpf.clone());
        hpf
    }
}

pub struct Bpf {
    tag: Tag,
    wave: Tag,
    s0: f32,
    s1: f32,
    s2: f32,
    s3: f32,
}

impl Bpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            s0: 0.0,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
        }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, rack: &Rack) -> bool {
        let ctrl = rack.controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {ctrl:?}"),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Bpf {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let x0 = rack.outputs[(self.wave, 0)];
        let cut_off = self.cutoff(rack);
        if self.off(rack) || cut_off > 20_000.0 {
            rack.outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(rack);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 - b2);
        let a1 = 0.0;
        let a2 = -a0;
        rack.outputs[(tag, 0)] =
            a0 * x0 + a1 * self.s0 + a2 * self.s1 - b1 * self.s2 - b2 * self.s3;
        self.s1 = self.s0;
        self.s0 = x0;
        self.s3 = self.s2;
        self.s2 = if rack.outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            rack.outputs[(tag, 0)]
        };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct BpfBuilder {
    wave: Tag,
    cut_off: Control,
    q: Control,
    off: Control,
}

impl BpfBuilder {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            cut_off: 25_000.0.into(),
            q: 0.707.into(),
            off: false.into(),
        }
    }

    build!(cut_off);
    build!(q);
    build!(off);

    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Bpf> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.cut_off;
        rack.controls[(n, 1)] = self.q;
        rack.controls[(n, 2)] = self.off;
        let bpf = arc_mutex(Bpf::new(n.into(), self.wave));
        rack.push(bpf.clone());
        bpf
    }
}

pub struct Notch {
    tag: Tag,
    wave: Tag,
    s0: f32,
    s1: f32,
    s2: f32,
    s3: f32,
}

impl Notch {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            s0: 0.0,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
        }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, rack: &Rack) -> bool {
        let ctrl = rack.controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {ctrl:?}"),
        }
    }
    pub fn set_off(&self, rack: &mut Rack, value: bool) {
        rack.controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Notch {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let x0 = rack.outputs[(self.wave, 0)];
        let cut_off = self.cutoff(rack);
        if self.off(rack) || cut_off > 20_000.0 {
            rack.outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(rack);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 + b2);
        let a1 = b1;
        rack.outputs[(tag, 0)] =
            a0 * x0 + a1 * self.s0 + a0 * self.s1 - b1 * self.s2 - b2 * self.s3;
        self.s1 = self.s0;
        self.s0 = x0;
        self.s3 = self.s2;
        self.s2 = if rack.outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            rack.outputs[(tag, 0)]
        };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct NotchBuilder {
    wave: Tag,
    cut_off: Control,
    q: Control,
    off: Control,
}

impl NotchBuilder {
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
            cut_off: 25_000.0.into(),
            q: 0.707.into(),
            off: false.into(),
        }
    }

    build!(cut_off);
    build!(q);
    build!(off);

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> ArcMutex<Notch> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.cut_off;
        controls[(n, 1)] = self.q;
        controls[(n, 2)] = self.off;
        let notch = arc_mutex(Notch::new(n.into(), self.wave));
        rack.push(notch.clone());
        notch
    }
}
/// Lowpass-Feedback Comb Filter
// https://ccrma.stanford.edu/~jos/pasp/Lowpass_Feedback_Comb_Filter.html
#[derive(Clone)]
pub struct Comb {
    tag: Tag,
    wave: Tag,
    s0: f32,
}

impl Comb {
    pub fn new<T: Into<Tag>>(tag: T, wave: Tag) -> Self {
        Self {
            tag: tag.into(),
            wave,
            s0: 0.0,
        }
    }
    props!(feedback, set_feedback, 0);
    props!(dampening, set_dampening, 1);
    props!(dampening_inverse, set_dampening_inverse, 2);
}

impl Signal for Comb {
    tag!();
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = rack.buffers.buffers(self.tag).get_max_delay();
        self.s0 = rack.outputs[(self.tag, 0)] * self.dampening_inverse(rack)
            + self.s0 * self.dampening(rack);
        let feedback = self.feedback(rack);
        rack.buffers
            .buffers_mut(self.tag)
            .push(rack.outputs[(self.wave, 0)] + self.s0 * feedback);
    }
}

#[derive(Clone)]
pub struct CombBuilder {
    wave: Tag,
    length: usize,
    feedback: Control,
    dampening: Control,
    dampening_inverse: Control,
}

impl CombBuilder {
    pub fn new(wave: Tag, length: usize) -> Self {
        Self {
            wave,
            length,
            feedback: 0.5.into(),
            dampening: 0.5.into(),
            dampening_inverse: 0.5.into(),
        }
    }

    build!(feedback);
    build!(dampening);
    build!(dampening_inverse);

    pub fn rack(&mut self, rack: &mut Rack) -> ArcMutex<Comb> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.feedback;
        rack.controls[(n, 1)] = self.dampening;
        rack.controls[(n, 2)] = self.dampening_inverse;
        let comb = arc_mutex(Comb::new(n, self.wave));
        rack.buffers
            .set_buffer(comb.lock().tag, RingBuffer::new(1, vec![0.0; self.length]));
        rack.push(comb.clone());
        comb
    }
}

#[derive(Debug, Copy, Clone)]
pub struct AllPass {
    tag: Tag,
    wave: Tag,
}

impl AllPass {
    pub fn new<T: Into<Tag>>(tag: T, wave: Tag) -> Self {
        Self {
            tag: tag.into(),
            wave,
        }
    }
}

impl Signal for AllPass {
    tag!();
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let input = rack.outputs[(self.wave, 0)];
        let delayed = rack.buffers.buffers(self.tag).get_max_delay();
        rack.outputs[(self.tag, 0)] = delayed - input;
        rack.buffers
            .buffers_mut(self.tag)
            .push(input + 0.5 * delayed);
    }
}

#[derive(Clone)]
pub struct AllPassBuilder {
    wave: Tag,
    length: usize,
}

impl AllPassBuilder {
    pub fn new(wave: Tag, length: usize) -> Self {
        Self { wave, length }
    }
    pub fn rack(&mut self, rack: &mut Rack, buffers: &mut Buffers) -> ArcMutex<AllPass> {
        let n = rack.num_modules();
        let allpass = arc_mutex(AllPass::new(n, self.wave));
        buffers.set_buffer(
            allpass.lock().tag,
            RingBuffer::new(1, vec![0.0; self.length]),
        );
        rack.push(allpass.clone());
        allpass
    }
}
