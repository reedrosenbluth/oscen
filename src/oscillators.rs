use super::graph::*;
use math::round::floor;
use rand::distributions::Uniform;
use rand::prelude::*;
use std::any::Any;
use std::{
    f64::consts::PI,
    ops::{Index, IndexMut},
};

/// A basic sine oscillator.
#[derive(Copy, Clone)]
pub struct SineOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SineOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: (0.0).into(),
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {
            tag: mk_tag(),
            hz,
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for SineOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        amplitude * (TAU * phase).sin()
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for SineOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SineOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SineOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SineOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SineOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

/// Saw wave oscillator.
#[derive(Copy, Clone)]
pub struct SawOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SawOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: (0.0).into(),
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {
            tag: mk_tag(),
            hz,
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for SawOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        let t = phase - 0.5;
        let s = -t - floor(0.5 - t, 0);
        if s < -0.499 {
            0.0
        } else {
            amplitude * 2.0 * s
        }
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for SawOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SawOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SawOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SawOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SawOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

/// Triangle wave oscillator.
#[derive(Copy, Clone)]
pub struct TriangleOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl TriangleOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: (0.0).into(),
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {
            tag: mk_tag(),
            hz,
            amplitude: (1.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for TriangleOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        let t = phase - 0.75;
        let saw_amp = 2. * (-t - floor(0.5 - t, 0));
        (2. * saw_amp.abs() - amplitude) * amplitude
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for TriangleOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("TriangleOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for TriangleOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("TriangleOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for TriangleOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

/// Square wave oscillator with a `duty_cycle` that takes values in (0, 1).
#[derive(Copy, Clone)]
pub struct SquareOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
    pub duty_cycle: In,
}

impl SquareOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: (0.0).into(),
            amplitude: (1.0).into(),
            phase: (0.0).into(),
            duty_cycle: (0.5).into(),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {
            tag: mk_tag(),
            hz,
            amplitude: (1.0).into(),
            phase: (0.0).into(),
            duty_cycle: (0.5).into(),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for SquareOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        let duty_cycle = In::val(graph, self.duty_cycle);
        let t = phase - floor(phase, 0);
        if t < 0.001 {
            0.0
        } else if t <= duty_cycle {
            amplitude
        } else {
            -amplitude
        }
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for SquareOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SquareOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SquareOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SquareOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SquareOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

#[derive(Clone)]
pub struct WhiteNoise {
    pub tag: Tag,
    pub amplitude: In,
    dist: Uniform<Real>,
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            amplitude: (1.0).into(),
            dist: Uniform::new_inclusive(-1.0, 1.0),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for WhiteNoise {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let mut rng = rand::thread_rng();
        let amplitude = In::val(graph, self.amplitude);
        self.dist.sample(&mut rng) * amplitude
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for WhiteNoise {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "amp" => &self.amplitude,
            _ => panic!("WhiteNoise does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for WhiteNoise {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "amp" => &mut self.amplitude,
            _ => panic!("WhiteNoise does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for WhiteNoise {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}
/// An oscillator used to modulate parameters that take values between 0 and 1,
/// based on a sinusoid.
#[derive(Copy, Clone)]
pub struct Osc01 {
    pub tag: Tag,
    pub hz: In,
    pub phase: In,
}

impl Osc01 {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: (0.0).into(),
            phase: (0.0).into(),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {
            tag: mk_tag(),
            hz,
            phase: (0.0).into(),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for Osc01 {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        0.5 * ((TAU * phase).sin() + 1.0)
    }
    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for Osc01 {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "phase" => &self.phase,
            _ => panic!("Osc01 does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Osc01 {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "phase" => &mut self.phase,
            _ => panic!("Osc01 does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for Osc01 {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

fn sinc(x: Real) -> Real {
    if x == 0.0 {
        return 1.0;
    }
    (PI * x).sin() / (PI * x)
}

/// Fourier series approximation for an oscillator. Optionally applies Lanczos Sigma
/// factor to eliminate ringing due to Gibbs phenomenon.
pub struct FourierOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    sines: Graph,
    pub lanczos: bool,
}

impl FourierOsc {
    pub fn new(coefficients: &[Real], lanczos: bool) -> Self {
        let sigma = if lanczos { 1.0 } else { 0.0 };
        let mut wwaves: Vec<ArcMutex<Sig>> = Vec::new();
        for (n, c) in coefficients.iter().enumerate() {
            let mut s = SineOsc::new();
            s.amplitude = (*c * sinc(sigma * n as Real / coefficients.len() as Real)).into();
            wwaves.push(arc(s));
        }
        FourierOsc {
            tag: mk_tag(),
            hz: (0.0).into(),
            amplitude: (1.0).into(),
            sines: Graph::new(wwaves),
            lanczos,
        }
    }
}

impl Signal for FourierOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amp = In::val(graph, self.amplitude);
        for (n, o) in self.sines.order.iter().enumerate() {
            if let Some(v) =
                self.sines.nodes.get_mut(o).unwrap().module
                    .lock()
                    .unwrap()
                    .as_any_mut()
                    .downcast_mut::<SineOsc>()
            {
                v.hz = (hz * n as Real).into();
            }
        }
        self.sines.signal(sample_rate);
        let out = self.sines.nodes.iter().fold(0., |acc, x| acc + x.1.output);
        amp * out
    }

    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for FourierOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            _ => panic!("FourierOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for FourierOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            _ => panic!("FourierOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for FourierOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value.into();
        }
    }
}

pub fn square_wave(n: u32, lanczos: bool) -> FourierOsc {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as Real);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::new(coefficients.as_ref(), lanczos)
}

pub fn triangle_wave(n: u32, lanczos: bool) -> FourierOsc {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as Real);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::new(coefficients.as_ref(), lanczos)
}

/// "pattern match" node on each oscillator type and set hz
pub fn set_hz(graph: &Graph, n: Tag, hz: Real) {
    SineOsc::set(graph, n, "hz", hz);
    SawOsc::set(graph, n, "hz", hz);
    TriangleOsc::set(graph, n, "hz", hz);
    SquareOsc::set(graph, n, "hz", hz);
    Osc01::set(graph, n, "hz", hz);
    FourierOsc::set(graph, n, "hz", hz);
}
