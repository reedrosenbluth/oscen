use super::signal::*;
use crate::{as_any_mut, std_signal};
use math::round::floor;
use rand::prelude::*;
use rand_distr::{StandardNormal, Uniform};
use std::any::Any;
use std::{
    f64::consts::PI,
    ops::{Index, IndexMut},
};

/// A `SynthModule` that emits 1.0 every `interval` seconds otherwise it emits
/// 0.0.
#[derive(Copy, Clone)]
pub struct Clock {
    tag: Tag,
    clock: u64,
    interval: Real,
}

impl Clock {
    pub fn new(interval: Real) -> Self {
        Self {
            tag: mk_tag(),
            clock: 0,
            interval,
        }
    }
}

impl Signal for Clock {
    std_signal!();
    fn signal(&mut self, _rack: &Rack, sample_rate: Real) -> Real {
        let interval = (self.interval * sample_rate) as u64;
        if self.clock == 0 {
            self.clock += 1;
            1.0
        } else {
            self.clock += 1;
            self.clock %= interval;
            0.0
        }
    }
}

/// A basic sine oscillator.
#[derive(Copy, Clone)]
pub struct SineOsc {
    tag: Tag,
    hz: In,
    amplitude: In,
    phase: In,
}

impl SineOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
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
    tag: Tag,
    hz: In,
    amplitude: In,
    phase: In,
}

impl SawOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
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
        if s < -0.5 {
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
    tag: Tag,
    hz: In,
    amplitude: In,
    phase: In,
}

impl TriangleOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
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

/// Square (Pulse) wave oscillator with a `duty_cycle` that takes values in (0, 1),
/// that determines the pulse width.
#[derive(Copy, Clone)]
pub struct SquareOsc {
    tag: Tag,
    hz: In,
    amplitude: In,
    phase: In,
    duty_cycle: In,
}

impl SquareOsc {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
            duty_cycle: (0.5).into(),
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
        self
    }

    pub fn duty_cycle<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.duty_cycle = arg.into();
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
        if t <= duty_cycle {
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

/// Choose between Normal(0,1) and Uniforem distributions for `WhiteNoise`.
#[derive(Copy, Clone)]
pub enum NoiseDistribution {
    StdNormal,
    Uni,
}

/// White noise oscillator.
#[derive(Copy, Clone)]
pub struct WhiteNoise {
    tag: Tag,
    amplitude: In,
    dist: NoiseDistribution,
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            amplitude: 1.into(),
            dist: NoiseDistribution::StdNormal,
        }
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }

    pub fn dist(&mut self, arg: NoiseDistribution) -> &mut Self {
        self.dist = arg;
        self
    }
}

impl Builder for WhiteNoise {}

impl Signal for WhiteNoise {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let amplitude = In::val(rack, self.amplitude);
        let mut rng = thread_rng();
        match self.dist {
            NoiseDistribution::Uni => {
                amplitude * Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng)
            }
            NoiseDistribution::StdNormal => amplitude * rng.sample::<f64, _>(StandardNormal),
        }
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

/// Pink noise oscillator.
// Paul Kellet's pk3 as in:
// paul.kellett@maxim.abel.co.uk, http://www.abel.co.uk/~maxim/ 
#[derive(Copy, Clone)]
pub struct PinkNoise {
    tag: Tag,
    b: [Real; 7],
    amplitude: In,
}

impl PinkNoise {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            b: [0.0; 7],
            amplitude: 1.into(),
        }
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
        self
    }
}

impl Builder for PinkNoise {}

impl Signal for PinkNoise {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        let mut rng = thread_rng();
        let white = Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng);
        self.b[0] = 0.99886 * self.b[0] + white * 0.0555179;
        self.b[1] = 0.99332 * self.b[1] + white * 0.0750759;
        self.b[2] = 0.96900 * self.b[2] + white * 0.1538520;
        self.b[3] = 0.86650 * self.b[3] + white * 0.3104856;
        self.b[4] = 0.55000 * self.b[4] + white * 0.5329522;
        self.b[5] = -0.7616 * self.b[5] - white * 0.0168980;
        let pink = self.b[0]
            + self.b[1]
            + self.b[2]
            + self.b[3]
            + self.b[4]
            + self.b[5]
            + self.b[6]
            + white * 0.5362;
        self.b[6] = white * 0.115926;
        pink * In::val(rack, self.amplitude)
    }
}

impl Index<&str> for PinkNoise {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "amp" => &self.amplitude,
            _ => panic!("PinkNoise does not have a field names: {}", index),
        }
    }
}

impl IndexMut<&str> for PinkNoise {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "amp" => &mut self.amplitude,
            _ => panic!("PinkNoise does not have a field named:  {}", index),
        }
    }
}

/// An oscillator used to modulate parameters that take values between 0 and 1,
/// based on a sinusoid.
#[derive(Copy, Clone)]
pub struct Osc01 {
    tag: Tag,
    hz: In,
    phase: In,
}

impl Osc01 {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            hz: 0.into(),
            phase: 0.into(),
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn phase<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.phase = arg.into();
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
    tag: Tag,
    hz: In,
    amplitude: In,
    sines: Rack,
    lanczos: bool,
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
            hz: 0.into(),
            amplitude: 1.into(),
            sines: Rack::new(wwaves),
            lanczos,
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn amplitude<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.amplitude = arg.into();
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
        let out = self.sines.modules.iter().fold(0., |acc, x| acc + x.1.output);
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

/// Square wave oscillator implemented as a fourier approximation.
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

/// Triangle wave oscillator implemented as a fourier approximation.
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