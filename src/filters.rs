use super::graph::*;
use std::any::Any;
use std::{f64::consts::SQRT_2, f64::consts::PI, ops::{Index, IndexMut}};

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
    pub fn new(tag: Tag, wave: Tag, cutoff_freq: In) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq,
            q: fix(1.0 / SQRT_2),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }
}

impl Signal for Lpf {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let x0 = graph.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(graph, self.cutoff_freq);
        let q = In::val(graph, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 + b1 + b2);
        let a1 = 2.0 * a0;
        a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2
    }

    fn tag(&self) -> Tag {
        self.tag
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

impl<'a> Set<'a> for Lpf {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn lpf_on(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Lpf>()
    {
        v.off = false;
    }
}

pub fn lpf_off(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Lpf>()
    {
        v.off = true;
    }
}

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
    pub fn new(tag: Tag, wave: Tag, cutoff_freq: In) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq,
            q: fix(1.0 / SQRT_2),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }
}

impl Signal for Hpf {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let x0 = graph.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(graph, self.cutoff_freq);
        let q = In::val(graph, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (2.0 * q - phi.sin()) / (2.0 * q + phi.sin());
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.25 * (1.0 - b1 + b2);
        let a1 = -2.0 * a0;
        a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2
    }

    fn tag(&self) -> Tag {
        self.tag
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

impl<'a> Set<'a> for Hpf {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn hpf_on(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Hpf>()
    {
        v.off = false;
    }
}

pub fn hpf_off(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Hpf>()
    {
        v.off = true;
    }
}

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
    pub fn new(tag: Tag, wave: Tag, cutoff_freq: In) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq,
            q: fix(1.0 / SQRT_2),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }
}

impl Signal for Bpf {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let x0 = graph.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(graph, self.cutoff_freq);
        let q = In::val(graph, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 - b2);
        let a1 = 0.0;
        let a2 = -a0;
        a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2
    }

    fn tag(&self) -> Tag {
        self.tag
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

impl<'a> Set<'a> for Bpf {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn bpf_on(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Bpf>()
    {
        v.off = false;
    }
}

pub fn bpf_off(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Bpf>()
    {
        v.off = true;
    }
}

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
    pub fn new(tag: Tag, wave: Tag, cutoff_freq: In) -> Self {
        Self {
            tag,
            wave,
            cutoff_freq,
            q: fix(1.0 / SQRT_2),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            off: false,
        }
    }
}

impl Signal for Notch {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let x0 = graph.output(self.wave);
        if self.off {
            return x0;
        }
        let cutoff_freq = In::val(graph, self.cutoff_freq);
        let q = In::val(graph, self.q);
        let phi = TAU * cutoff_freq / sample_rate;
        let b2 = (PI / 4.0 - phi / (2.0 * q)).tan();
        let b1 = -(1.0 + b2) * phi.cos();
        let a0 = 0.5 * (1.0 + b2);
        let a1 = b1;
        let a2 = a0;
        a0 * x0 + a1 * self.x1 + a0 * self.x2 - b1 * self.y1 - b2 * self.y2
    }

    fn tag(&self) -> Tag {
        self.tag
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

impl<'a> Set<'a> for Notch {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn notch_on(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Notch>()
    {
        v.off = false;
    }
}

pub fn notch_off(graph: &Graph, n: Tag) {
    if let Some(v) = graph.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<Notch>()
    {
        v.off = true;
    }
}

/// Lowpass-Feedback Comb Filter
/// https://ccrma.stanford.edu/~jos/pasp/Lowpass_Feedback_Comb_Filter.html
pub struct Comb {
    pub tag: Tag,
    pub wave: Tag,
    buffer: Vec<Real>,
    index: usize,
    pub feedback: Real,
    pub filter_state: Real,
    pub dampening: Real,
    pub dampening_inverse: Real,
}

impl Comb {
    pub fn new(wave: Tag, length: usize) -> Self {
        Self {
            tag: mk_tag(),
            wave,
            buffer: vec![0.0; length],
            index: 0,
            feedback: 0.5,
            filter_state: 0.0,
            dampening: 0.5,
            dampening_inverse: 0.5,
        }
    }

    pub fn wrapped(wave: Tag, length: usize) -> ArcMutex<Self> {
        arc(Self::new(wave, length))
    }
}

impl Signal for Comb {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let input = graph.output(self.wave);
        let output = self.buffer[self.index] as Real;
        self.filter_state = output * self.dampening_inverse + self.filter_state * self.dampening;
        self.buffer[self.index] = input + (self.filter_state * self.feedback) as Real;
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0
        }
        output as Real
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

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

    pub fn wrapped(wave: Tag, length: usize) -> ArcMutex<Self> {
        arc(Self::new(wave, length))
    }
}

impl Signal for AllPass {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let input = graph.output(self.wave);
        let delayed = self.buffer[self.index];
        let output = delayed - input;
        self.buffer[self.index] = input + (0.5 * delayed) as Real;
        self.index += 1;
        if self.index == self.buffer.len() {
            self.index = 0
        }
        output as Real
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}
