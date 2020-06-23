use super::signal::*;
use crate::{as_any_mut, std_signal};
use std::any::Any;
use std::{
    f64::consts::PI,
    f64::consts::SQRT_2,
    ops::{Index, IndexMut},
};

#[derive(Clone)]
pub struct Lpf {
    pub tag: Tag,
    pub wave: Tag,
    pub cutoff_freq: In,
    pub q: In,
    x1: Real,
    x2: Real,
    y1: Real,
    y2: Real,
    pub off: bool,
}

impl Lpf {
    pub fn new(wave: Tag) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            cutoff_freq: 22050.into(),
            q: (1.0 / SQRT_2).into(),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.cutoff_freq = arg.into();
        self
    }

    pub fn q<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.q = arg.into();
        self
    }
}

impl Builder for Lpf {}

impl Signal for Lpf {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let x0 = rack.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(rack, self.cutoff_freq);
        let q = In::val(rack, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 + b1 + b2);
        let a1 = 2.0 * a0;
        let amp = a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = if amp.is_nan() { 0.0 } else { amp };
        amp
    }
}

impl Index<&str> for Lpf {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "cutoff_freq" => &self.cutoff_freq,
            "q" => &self.q,
            _ => panic!("Lpf does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Lpf {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "cutoff_freq" => &mut self.cutoff_freq,
            "q" => &mut self.q,
            _ => panic!("Lpf does not have a field named: {}", index),
        }
    }
}

pub fn lpf_on(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Lpf>()
    {
        v.off = false;
    }
}

pub fn lpf_off(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Lpf>()
    {
        v.off = true;
    }
}

#[derive(Clone)]
pub struct Hpf {
    pub tag: Tag,
    pub wave: Tag,
    pub cutoff_freq: In,
    pub q: In,
    x1: Real,
    x2: Real,
    y1: Real,
    y2: Real,
    pub off: bool,
}

impl Hpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq: 22050.into(),
            q: (1.0 / SQRT_2).into(),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.cutoff_freq = arg.into();
        self
    }

    pub fn q<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.q = arg.into();
        self
    }
}

impl Builder for Hpf {}

impl Signal for Hpf {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let x0 = rack.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(rack, self.cutoff_freq);
        let q = In::val(rack, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 - b1 + b2);
        let a1 = -2.0 * a0;
        let amp = a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = if amp.is_nan() { 0.0 } else { amp };
        amp
    }
}

impl Index<&str> for Hpf {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "cutoff_freq" => &self.cutoff_freq,
            "q" => &self.q,
            _ => panic!("Hpf does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Hpf {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "cutoff_freq" => &mut self.cutoff_freq,
            "q" => &mut self.q,
            _ => panic!("Hpf does not have a field named: {}", index),
        }
    }
}

pub fn hpf_on(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Hpf>()
    {
        v.off = false;
    }
}

pub fn hpf_off(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Hpf>()
    {
        v.off = true;
    }
}

#[derive(Clone)]
pub struct Bpf {
    pub tag: Tag,
    pub wave: Tag,
    pub cutoff_freq: In,
    pub q: In,
    x1: Real,
    x2: Real,
    y1: Real,
    y2: Real,
    pub off: bool,
}

impl Bpf {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq: 22050.into(),
            q: (1.0 / SQRT_2).into(),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.cutoff_freq = arg.into();
        self
    }

    pub fn q<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.q = arg.into();
        self
    }
}

impl Builder for Bpf {}

impl Signal for Bpf {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let x0 = rack.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(rack, self.cutoff_freq);
        let q = In::val(rack, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 - b2);
        let a1 = 0.0;
        let a2 = -a0;
        let amp = a0 * x0 + a1 * self.x1 + a2 * self.x2 - b1 * self.y1 - b2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = if amp.is_nan() { 0.0 } else { amp };
        amp
    }
}

impl Index<&str> for Bpf {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "cutoff_freq" => &self.cutoff_freq,
            "q" => &self.q,
            _ => panic!("Bpf does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Bpf {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "cutoff_freq" => &mut self.cutoff_freq,
            "q" => &mut self.q,
            _ => panic!("Bpf does not have a field named: {}", index),
        }
    }
}

pub fn bpf_on(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Bpf>()
    {
        v.off = false;
    }
}

pub fn bpf_off(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Bpf>()
    {
        v.off = true;
    }
}

#[derive(Clone)]
pub struct Notch {
    pub tag: Tag,
    pub wave: Tag,
    pub cutoff_freq: In,
    pub q: In,
    x1: Real,
    x2: Real,
    y1: Real,
    y2: Real,
    pub off: bool,
}

impl Notch {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq: 22050.into(),
            q: (1.0 / SQRT_2).into(),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }

    pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.cutoff_freq = arg.into();
        self
    }

    pub fn q<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.q = arg.into();
        self
    }
}

impl Builder for Notch {}

impl Signal for Notch {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let x0 = rack.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(rack, self.cutoff_freq);
        let q = In::val(rack, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 + b2);
        let a1 = b1;
        let amp = a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = if amp.is_nan() { 0.0 } else { amp };
        amp
    }
}

impl Index<&str> for Notch {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "cutoff_freq" => &self.cutoff_freq,
            "q" => &self.q,
            _ => panic!("Notch does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Notch {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "cutoff_freq" => &mut self.cutoff_freq,
            "q" => &mut self.q,
            _ => panic!("Notch does not have a field named: {}", index),
        }
    }
}

pub fn notch_on(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Notch>()
    {
        v.off = false;
    }
}

pub fn notch_off(rack: &mut Rack, n: Tag) {
    if let Some(v) = rack
        .nodes
        .get_mut(&n)
        .unwrap()
        .module
        .as_any_mut()
        .downcast_mut::<Notch>()
    {
        v.off = true;
    }
}

/// Lowpass-Feedback Comb Filter
/// https://ccrma.stanford.edu/~jos/pasp/Lowpass_Feedback_Comb_Filter.html
#[derive(Clone)]
pub struct Comb {
    pub tag: Tag,
    pub wave: Tag,
    buffer: Vec<Real>,
    index: usize,
    pub feedback: In,
    pub filter_state: Real,
    pub dampening: In,
    pub dampening_inverse: In,
}

impl Comb {
    pub fn new(wave: Tag, length: usize) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            buffer: vec![0.0; length],
            index: 0,
            feedback: (0.5).into(),
            filter_state: 0.0,
            dampening: (0.5).into(),
            dampening_inverse: (0.5).into(),
        }
    }

    pub fn feedback<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.feedback = arg.into();
        self
    }

    pub fn dampening<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.dampening = arg.into();
        self
    }

    pub fn dampening_inverse<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.dampening_inverse = arg.into();
        self
    }
}

impl Builder for Comb {}

impl Signal for Comb {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let feedback = In::val(rack, self.feedback);
        let dampening = In::val(rack, self.dampening);
        let dampening_inverse = In::val(rack, self.dampening_inverse);
        let input = rack.output(self.wave);
        let output = self.buffer[self.index];
        self.filter_state = output * dampening_inverse + self.filter_state * dampening;
        self.buffer[self.index] = input + (self.filter_state * feedback);
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0
        }
        output
    }
}

impl Index<&str> for Comb {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "feedback" => &self.feedback,
            "damping" => &self.dampening,
            "damping_inverse" => &self.dampening_inverse,
            _ => panic!("Comb does not have a field named: {}", index),
        }
    }
}

impl IndexMut<&str> for Comb {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "feedback" => &mut self.feedback,
            "damping" => &mut self.dampening,
            "damping_inverse" => &mut self.dampening_inverse,
            _ => panic!("Comb does not have a field named: {}", index),
        }
    }
}

#[derive(Clone)]
pub struct AllPass {
    pub tag: Tag,
    pub wave: Tag,
    buffer: Vec<Real>,
    index: usize,
}

impl AllPass {
    pub fn new(wave: Tag, length: usize) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            buffer: vec![0.0; length],
            index: 0,
        }
    }
}

impl Builder for AllPass {}

impl Signal for AllPass {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let input = rack.output(self.wave);
        let delayed = self.buffer[self.index];
        let output = delayed - input;
        self.buffer[self.index] = input + (0.5 * delayed) as Real;
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0
        }
        output as Real
    }
}
