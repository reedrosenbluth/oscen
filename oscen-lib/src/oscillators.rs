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

/// An synth module that returns a constant In value. Useful for example to
/// multiply or add constants to oscillators.
#[derive(Copy, Clone)]
pub struct ConstOsc {
    tag: Tag,
    value: In,
    out: Real,
}

impl ConstOsc {
    pub fn new(id_gen: &mut IdGen, value: In) -> Self {
        Self {
            tag: id_gen.id(),
            value,
            out: 0.0,
        }
    }

    pub fn value<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.value = arg.into();
        self
    }
}

impl Builder for ConstOsc {}

impl Signal for ConstOsc {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        self.out = In::val(rack, self.value);
        self.out
    }
}

/// A `SynthModule` that emits 1.0 every `interval` seconds otherwise it emits
/// 0.0.
#[derive(Copy, Clone)]
pub struct Clock {
    tag: Tag,
    clock: u64,
    interval: Real,
    out: Real,
}

impl Clock {
    pub fn new(id_gen: &mut IdGen, interval: Real) -> Self {
        Self {
            tag: id_gen.id(),
            clock: 0,
            interval,
            out: 0.0,
        }
    }
}

impl Signal for Clock {
    std_signal!();
    fn signal(&mut self, _rack: &Rack, sample_rate: Real) -> Real {
        let interval = (self.interval * sample_rate) as u64;
        if self.clock == 0 {
            self.clock += 1;
            self.out = 1.0;
        } else {
            self.clock += 1;
            self.clock %= interval;
            self.out = 0.0;
        }
        self.out
    }
}

pub type SignalFn = fn(Real, Real) -> Real;

#[derive(Copy, Clone)]
pub struct Oscillator {
    tag: Tag,
    hz: In,
    amplitude: In,
    phase: In,
    arg: In,
    signal_fn: fn(Real, Real) -> Real,
    out: Real,
}

impl Oscillator {
    pub fn new(id_gen: &mut IdGen, signal_fn: SignalFn) -> Self {
        Self {
            tag: id_gen.id(),
            hz: 0.into(),
            amplitude: 1.into(),
            phase: 0.into(),
            arg: 0.into(),
            signal_fn,
            out: 0.0,
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

    pub fn arg<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.arg = arg.into();
        self
    }
}

impl Builder for Oscillator {}

impl Signal for Oscillator {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let hz = In::val(rack, self.hz);
        let amplitude = In::val(rack, self.amplitude);
        if hz == 0.0 {
            self.out = amplitude;
            return self.out;
        }
        let phase = In::val(rack, self.phase);
        let arg = In::val(rack, self.arg);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        self.out = amplitude * (self.signal_fn)(phase, arg);
        self.out
    }
}

impl Index<&str> for Oscillator {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            "arg" => &self.arg,
            _ => panic!("StandardOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Oscillator {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            "arg" => &mut self.arg,
            _ => panic!("StandardOsc does not have a field named:  {}", index),
        }
    }
}

pub fn sine_osc(phase: Real, _: Real) -> Real {
    (phase * TAU).sin()
}

pub fn square_osc(phase: Real, duty_cycle: Real) -> Real {
    let t = phase - floor(phase, 0);
    if t <= duty_cycle {
        1.0
    } else {
        -1.0
    }
}

pub fn saw_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.5;
    let s = -t - floor(0.5 - t, 0);
    if s < -0.5 {
        0.0
    } else {
        2.0 * s
    }
}

pub fn triangle_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.75;
    let saw_amp = 2. * (-t - floor(0.5 - t, 0));
    2. * saw_amp.abs() - 1.0
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
    out: Real,
}

impl WhiteNoise {
    pub fn new(id_gen: &mut IdGen) -> Self {
        Self {
            tag: id_gen.id(),
            amplitude: 1.into(),
            dist: NoiseDistribution::StdNormal,
            out: 0.0,
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
                self.out = amplitude * Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng)
            }
            NoiseDistribution::StdNormal => {
                self.out = amplitude * rng.sample::<f64, _>(StandardNormal)
            }
        }
        self.out
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
    out: Real,
}

impl PinkNoise {
    pub fn new(id_gen: &mut IdGen) -> Self {
        Self {
            tag: id_gen.id(),
            b: [0.0; 7],
            amplitude: 1.into(),
            out: 0.0,
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
        self.out = pink * In::val(rack, self.amplitude);
        self.out
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
    out: Real,
}

impl Osc01 {
    pub fn new(id_gen: &mut IdGen) -> Self {
        Self {
            tag: id_gen.id(),
            hz: 0.into(),
            phase: 0.into(),
            out: 0.0,
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
        self.out = 0.5 * ((TAU * phase).sin() + 1.0);
        self.out
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
    out: Real,
}

impl FourierOsc {
    pub fn new(id_gen: &mut IdGen, coefficients: &[Real], lanczos: bool) -> Self {
        let sigma = lanczos as i32;
        let mut wwaves: Vec<ArcMutex<Sig>> = Vec::new();
        let mut id = IdGen::new();
        for (n, c) in coefficients.iter().enumerate() {
            let mut s = Oscillator::new(&mut id, sine_osc);
            s.amplitude =
                (*c * sinc(sigma as Real * n as Real / coefficients.len() as Real)).into();
            wwaves.push(arc(s));
        }
        FourierOsc {
            tag: id_gen.id(),
            hz: 0.into(),
            amplitude: 1.into(),
            sines: Rack::new().modules(wwaves).build(),
            lanczos,
            out: 0.0,
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
            if let Some(v) = node.lock().as_any_mut().downcast_mut::<Oscillator>() {
                v.hz = (hz * n as Real).into();
            }
        }
        self.sines.signal(sample_rate);
        let out = self.sines.0.iter().fold(0., |acc, x| acc + x.out());
        self.out = amp * out;
        self.out
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
pub fn square_wave(id_gen: &mut IdGen, n: u32, lanczos: bool) -> FourierOsc {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as Real);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::new(id_gen, coefficients.as_ref(), lanczos)
}

/// Triangle wave oscillator implemented as a fourier approximation.
pub fn triangle_wave(id_gen: &mut IdGen, n: u32, lanczos: bool) -> FourierOsc {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as Real);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::new(id_gen, coefficients.as_ref(), lanczos)
}
