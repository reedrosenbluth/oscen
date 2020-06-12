use super::signal::*;
use crate::{as_any_mut, std_signal};
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
            hz: In::zero(),
            amplitude: In::one(),
            phase: In::zero(),
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }

    pub fn phase(&mut self, arg: In) -> &mut Self {
        self.phase = arg;
        self
    }
}

impl Builder for SineOsc {}

impl Signal for SineOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        let phase = In::val(rack, self.phase);
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
            hz: In::zero(),
            amplitude: In::one(),
            phase: In::zero(),
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }

    pub fn phase(&mut self, arg: In) -> &mut Self {
        self.phase = arg;
        self
    }
}

impl Builder for SawOsc {}

impl Signal for SawOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        let phase = In::val(rack, self.phase);
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
            hz: In::zero(),
            amplitude: In::one(),
            phase: In::zero(),
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }

    pub fn phase(&mut self, arg: In) -> &mut Self {
        self.phase = arg;
        self
    }
}

impl Builder for TriangleOsc {}

impl Signal for TriangleOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        let phase = In::val(rack, self.phase);
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
            hz: In::zero(),
            amplitude: In::one(),
            phase: In::zero(),
            duty_cycle: (0.5).into(),
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }

    pub fn phase(&mut self, arg: In) -> &mut Self {
        self.phase = arg;
        self
    }
}

impl Builder for SquareOsc {}

impl Signal for SquareOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        let phase = In::val(rack, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        let duty_cycle = In::val(rack, self.duty_cycle);
        let t = phase - floor(phase, 0);
        if t < 0.001 {
            0.0
        } else if t <= duty_cycle {
            amplitude
        } else {
            -amplitude
        }
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

#[derive(Copy, Clone)]
pub struct WhiteNoise {
    pub tag: Tag,
    pub amplitude: In,
    dist: Uniform<Real>,
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            amplitude: In::one(),
            dist: Uniform::new_inclusive(-1.0, 1.0),
        }
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }
}

impl Builder for WhiteNoise {}

impl Signal for WhiteNoise {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let mut rng = rand::thread_rng();
        let amplitude = In::val(rack, self.amplitude);
        self.dist.sample(&mut rng) * amplitude
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
            hz: In::zero(),
            phase: In::zero(),
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn phase(&mut self, arg: In) -> &mut Self {
        self.phase = arg;
        self
    }
}

impl Builder for Osc01 {}

impl Signal for Osc01 {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let phase = In::val(rack, self.phase);
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

fn sinc(x: Real) -> Real {
    if x == 0.0 {
        return 1.0;
    }
    (PI * x).sin() / (PI * x)
}

/// Fourier series approximation for an oscillator. Optionally applies Lanczos Sigma
/// factor to eliminate ringing due to Gibbs phenomenon.
#[derive(Clone)]
pub struct FourierOsc {
    pub tag: Tag,
    pub hz: In,
    pub amplitude: In,
    sines: Rack,
    pub lanczos: bool,
}

impl FourierOsc {
    pub fn new(coefficients: &[Real], lanczos: bool) -> Self {
        let sigma = lanczos as i32;
        let mut wwaves: Vec<ArcMutex<Sig>> = Vec::new();
        for (n, c) in coefficients.iter().enumerate() {
            let mut s = SineOsc::new();
            s.amplitude =
                (*c * sinc(sigma as Real * n as Real / coefficients.len() as Real)).into();
            wwaves.push(arc(s));
        }
        FourierOsc {
            tag: mk_tag(),
            hz: In::zero(),
            amplitude: In::one(),
            sines: Rack::new(wwaves),
            lanczos,
        }
    }

    pub fn hz(&mut self, arg: In) -> &mut Self {
        self.hz = arg;
        self
    }

    pub fn amplitude(&mut self, arg: In) -> &mut Self {
        self.amplitude = arg;
        self
    }

    pub fn lanczos(&mut self, arg: bool) -> &mut Self {
        self.lanczos = arg;
        self
    }
}

impl Builder for FourierOsc {}

impl Signal for FourierOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amp = In::val(rack, self.amplitude);
        for (n, node) in self.sines.iter().enumerate() {
            if let Some(v) = node
                .module
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
