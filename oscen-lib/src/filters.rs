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
            cut_off: 25_000.into(),
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
            cut_off: 25_000.into(),
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
            cut_off: 25_000.into(),
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
            cut_off: 25_000.into(),
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
// Lowpass-Feedback Comb Filter
// https://ccrma.stanford.edu/~jos/pasp/Lowpass_Feedback_Comb_Filter.html
// #[derive(Clone)]
// pub struct Comb {
//     tag: Tag,
//     wave: Tag,
//     buffer: Vec<Real>,
//     index: usize,
//     feedback: In,
//     filter_state: Real,
//     dampening: In,
//     dampening_inverse: In,
//     out: Real,
// }

// impl Comb {
//     pub fn new(id_gen: &mut IdGen, wave: Tag, length: usize) -> Self {
//         Self {
//             tag: id_gen.id(),
//             wave,
//             buffer: vec![0.0; length],
//             index: 0,
//             feedback: (0.5).into(),
//             filter_state: 0.0,
//             dampening: (0.5).into(),
//             dampening_inverse: (0.5).into(),
//             out: 0.0,
//         }
//     }

//     pub fn wave(&mut self, arg: Tag) -> &mut Self {
//         self.wave = arg;
//         self
//     }

//     pub fn feedback<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.feedback = arg.into();
//         self
//     }

//     pub fn dampening<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.dampening = arg.into();
//         self
//     }

//     pub fn dampening_inverse<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.dampening_inverse = arg.into();
//         self
//     }
// }

// impl Builder for Comb {}

// impl Signal for Comb {
//     std_signal!();
//     fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
//         let feedback = In::val(rack, self.feedback);
//         let dampening = In::val(rack, self.dampening);
//         let dampening_inverse = In::val(rack, self.dampening_inverse);
//         let input = rack.output(self.wave);
//         self.out = self.buffer[self.index];
//         self.filter_state = self.out * dampening_inverse + self.filter_state * dampening;
//         self.buffer[self.index] = input + (self.filter_state * feedback);
//         self.index += 1;
//         if self.index == self.buffer.len() {
//             self.index = 0
//         }
//         self.out
//     }
// }

// impl Index<&str> for Comb {
//     type Output = In;

//     fn index(&self, index: &str) -> &Self::Output {
//         match index {
//             "feedback" => &self.feedback,
//             "damping" => &self.dampening,
//             "damping_inverse" => &self.dampening_inverse,
//             _ => panic!("Comb does not have a field named: {}", index),
//         }
//     }
// }

// impl IndexMut<&str> for Comb {
//     fn index_mut(&mut self, index: &str) -> &mut Self::Output {
//         match index {
//             "feedback" => &mut self.feedback,
//             "damping" => &mut self.dampening,
//             "damping_inverse" => &mut self.dampening_inverse,
//             _ => panic!("Comb does not have a field named: {}", index),
//         }
//     }
// }

// #[derive(Clone)]
// pub struct AllPass {
//     tag: Tag,
//     wave: Tag,
//     buffer: Vec<Real>,
//     index: usize,
//     out: Real,
// }

// impl AllPass {
//     pub fn new(id_gen: &mut IdGen, wave: Tag, length: usize) -> Self {
//         Self {
//             tag: id_gen.id(),
//             wave,
//             buffer: vec![0.0; length],
//             index: 0,
//             out: 0.0,
//         }
//     }

//     pub fn wave(&mut self, arg: Tag) -> &mut Self {
//         self.wave = arg;
//         self
//     }
// }

// impl Builder for AllPass {}

// impl Signal for AllPass {
//     std_signal!();
//     fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
//         let input = rack.output(self.wave);
//         let delayed = self.buffer[self.index];
//         let output = delayed - input;
//         self.buffer[self.index] = input + (0.5 * delayed) as Real;
//         self.index += 1;
//         if self.index == self.buffer.len() {
//             self.index = 0
//         }
//         self.out = output as Real;
//         self.out
//     }
// }
