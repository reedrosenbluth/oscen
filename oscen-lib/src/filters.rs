use crate::rack::*;
use crate::{build, props, tag};
use std::f32::consts::PI;
use std::sync::Arc;

#[derive(Debug, Copy, Clone)]
pub struct Lpf {
    tag: Tag,
    wave: Tag,
}

impl Lpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, controls: &Controls) -> bool {
        let ctrl = controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {:?}", ctrl),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Lpf {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let x0 = outputs[(self.wave, 0)];
        let cut_off = self.cutoff(controls, outputs);
        if self.off(controls) || cut_off > 20_000.0 {
            outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(controls, outputs);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 + b1 + b2);
        let a1 = 2.0 * a0;
        outputs[(tag, 0)] = a0 * x0 + a1 * state[(tag, 0)] + a0 * state[(tag, 1)]
            - b1 * state[(tag, 2)]
            - b2 * state[(tag, 3)];
        state[(tag, 1)] = state[(tag, 0)];
        state[(tag, 0)] = x0;
        state[(tag, 3)] = state[(tag, 2)];
        state[(tag, 2)] = if outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            outputs[(tag, 0)]
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

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Lpf> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.cut_off;
        controls[(n, 1)] = self.q;
        controls[(n, 2)] = self.off;
        let lpf = Arc::new(Lpf::new(n.into(), self.wave));
        rack.push(lpf.clone());
        lpf
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Hpf {
    tag: Tag,
    wave: Tag,
}

impl Hpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, controls: &Controls) -> bool {
        let ctrl = controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {:?}", ctrl),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Hpf {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let x0 = outputs[(self.wave, 0)];
        let cut_off = self.cutoff(controls, outputs);
        if self.off(controls) || cut_off > 20_000.0 {
            outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(controls, outputs);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 - b1 + b2);
        let a1 = -2.0 * a0;
        outputs[(tag, 0)] = a0 * x0 + a1 * state[(tag, 0)] + a0 * state[(tag, 1)]
            - b1 * state[(tag, 2)]
            - b2 * state[(tag, 3)];
        state[(tag, 1)] = state[(tag, 0)];
        state[(tag, 0)] = x0;
        state[(tag, 3)] = state[(tag, 2)];
        state[(tag, 2)] = if outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            outputs[(tag, 0)]
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

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Hpf> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.cut_off;
        controls[(n, 1)] = self.q;
        controls[(n, 2)] = self.off;
        let hpf = Arc::new(Hpf::new(n.into(), self.wave));
        rack.push(hpf.clone());
        hpf
    }
}

pub struct Bpf {
    tag: Tag,
    wave: Tag,
}

impl Bpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, controls: &Controls) -> bool {
        let ctrl = controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {:?}", ctrl),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Bpf {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let x0 = outputs[(self.wave, 0)];
        let cut_off = self.cutoff(controls, outputs);
        if self.off(controls) || cut_off > 20_000.0 {
            outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(controls, outputs);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 - b2);
        let a1 = 0.0;
        let a2 = -a0;
        outputs[(tag, 0)] = a0 * x0 + a1 * state[(tag, 0)] + a2 * state[(tag, 1)]
            - b1 * state[(tag, 2)]
            - b2 * state[(tag, 3)];
        state[(tag, 1)] = state[(tag, 0)];
        state[(tag, 0)] = x0;
        state[(tag, 3)] = state[(tag, 2)];
        state[(tag, 2)] = if outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            outputs[(tag, 0)]
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

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Bpf> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.cut_off;
        controls[(n, 1)] = self.q;
        controls[(n, 2)] = self.off;
        let bpf = Arc::new(Bpf::new(n.into(), self.wave));
        rack.push(bpf.clone());
        bpf
    }
}

pub struct Notch {
    tag: Tag,
    wave: Tag,
}

impl Notch {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
    }
    props!(cutoff, set_cutoff, 0);
    props!(q, set_q, 1);
    pub fn off(&self, controls: &Controls) -> bool {
        let ctrl = controls[(self.tag, 2)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("off must be a bool, not {:?}", ctrl),
        }
    }
    pub fn set_off(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 2)] = value.into();
    }
}

impl Signal for Notch {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let x0 = outputs[(self.wave, 0)];
        let cut_off = self.cutoff(controls, outputs);
        if self.off(controls) || cut_off > 20_000.0 {
            outputs[(self.tag, 0)] = x0;
            return;
        }
        let tag = self.tag;
        let q = self.q(controls, outputs);
        let phi = 2.0 * PI * cut_off / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 + b2);
        let a1 = b1;
        outputs[(tag, 0)] = a0 * x0 + a1 * state[(tag, 0)] + a0 * state[(tag, 1)]
            - b1 * state[(tag, 2)]
            - b2 * state[(tag, 3)];
        state[(tag, 1)] = state[(tag, 0)];
        state[(tag, 0)] = x0;
        state[(tag, 3)] = state[(tag, 2)];
        state[(tag, 2)] = if outputs[(tag, 0)].is_nan() {
            0.0
        } else {
            outputs[(tag, 0)]
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

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Notch> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.cut_off;
        controls[(n, 1)] = self.q;
        controls[(n, 2)] = self.off;
        let notch = Arc::new(Notch::new(n.into(), self.wave));
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
}

impl Comb {
    pub fn new<T: Into<Tag>>(tag: T, wave: Tag) -> Self {
        Self {
            tag: tag.into(),
            wave,
        }
    }
    props!(feedback, set_feedback, 0);
    props!(dampening, set_dampening, 1);
    props!(dampening_inverse, set_dampening_inverse, 2);
}

impl Signal for Comb {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let out = buffers.buffers(self.tag).get(0.0);
        state[(self.tag, 0)] = out * self.dampening_inverse(controls, outputs)
            + state[(self.tag, 0)] * self.dampening(controls, outputs);
        let input = outputs[(self.wave, 0)];
        buffers
            .buffers_mut(self.tag)
            .push(input + state[(self.tag, 0)] * self.feedback(controls, outputs));
    }
}

#[derive(Clone)]
pub struct CombBuilder {
    wave: Tag,
    buffer: RingBuffer,
    length: usize,
    feedback: Control,
    dampening: Control,
    dampening_inverse: Control,
}

impl CombBuilder {
    pub fn new(wave: Tag, buffer: RingBuffer, length: usize) -> Self {
        Self {
            wave,
            buffer,
            length,
            feedback: 0.5.into(),
            dampening: 0.5.into(),
            dampening_inverse: 0.5.into(),
        }
    }

    build!(feedback);
    build!(dampening);
    build!(dampening_inverse);

    pub fn rack(
        &mut self,
        rack: &mut Rack,
        controls: &mut Controls,
        buffers: &mut Buffers,
    ) -> Arc<Comb> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.feedback;
        controls[(n, 1)] = self.dampening;
        controls[(n, 2)] = self.dampening_inverse;
        self.buffer.resize(self.length);
        self.buffer.set_write_pos(1);
        let comb = Arc::new(Comb::new(n, self.wave));
        buffers.set_buffer(comb.tag, self.buffer.clone());
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
    fn signal(
        &self,
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let input = outputs[(self.wave, 0)];
        let delayed = buffers.buffers(self.tag).get(0.0);
        outputs[(self.tag, 0)] = delayed - input;
        buffers.buffers_mut(self.tag).push(input + 0.5 * delayed);
    }
}

#[derive(Clone)]
pub struct AllPassBuilder {
    wave: Tag,
    buffer: RingBuffer,
    length: usize,
}

impl AllPassBuilder {
    pub fn new(wave: Tag, buffer: RingBuffer, length: usize) -> Self {
        Self {
            wave,
            buffer,
            length,
        }
    }
    pub fn rack(&mut self, rack: &mut Rack, buffers: &mut Buffers) -> Arc<AllPass> {
        let n = rack.num_modules();
        let allpass = Arc::new(AllPass::new(n, self.wave));
        self.buffer.resize(self.length);
        self.buffer.set_write_pos(1);
        buffers.set_buffer(allpass.tag, self.buffer.clone());
        rack.push(allpass.clone());
        allpass
    }
}
