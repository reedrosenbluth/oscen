use crate::rack::*;
use crate::{tag, build, props};
use math::round::floor;
use rand::prelude::*;
use rand_distr::{StandardNormal, Uniform};
use std::f32::consts;

const TAU: f32 = 2.0 * consts::PI;

pub struct OscBuilder {
    signal_fn: fn(Real, Real) -> Real,
    phase: Control,
    hz: Control,
    amplitude: Control,
    arg: Control,
}

/// A standard oscillator that has phase, hz, and amp. Pass in a signal function
/// to operate on the phase and an optional extra argument.
#[derive(Clone)]
pub struct Oscillator {
    tag: Tag,
    phase: Control,
    signal_fn: fn(Real, Real) -> Real,
}

impl OscBuilder {
    pub fn new(signal_fn: fn(Real, Real) -> Real) -> Self {
        Self {
            signal_fn,
            phase: 0.into(),
            hz: 0.into(),
            amplitude: 1.into(),
            arg: 0.5.into(),
        }
    }
    build!(phase);
    build!(hz);
    build!(amplitude);
    build!(arg);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<Oscillator> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.hz;
        controls[(tag, 1)] = self.amplitude;
        controls[(tag, 2)] = self.arg;
        let osc = Box::new(Oscillator::new(tag, self.signal_fn));
        rack.push(osc.clone());
        osc
    }
}

pub fn sine_osc(phase: Real, _: Real) -> Real {
    (phase * TAU).sin()
}

pub fn square_osc(phase: Real, duty_cycle: Real) -> Real {
    let t = phase - phase.floor();
    if t <= duty_cycle {
        1.0
    } else {
        -1.0
    }
}

pub fn saw_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.5;
    let s = -t - floor(0.5 - t as f64, 0) as f32;
    if s < -0.5 {
        0.0
    } else {
        2.0 * s
    }
}

pub fn triangle_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.75;
    let saw_amp = 2. * (-t - floor(0.5 - t as f64, 0) as f32);
    2.0 * saw_amp.abs() - 1.0
}

impl Oscillator {
    pub fn new(tag: Tag, signal_fn: fn(Real, Real) -> Real) -> Self {
        Self {
            tag,
            phase: 0.into(),
            signal_fn,
        }
    }
    pub fn phase(&self, outputs: &Outputs) -> Real {
        outputs.value(self.phase).expect("phase must be Control")
    }
    pub fn set_phase(&mut self, value: Control) {
        self.phase = value;
    }
    props!(hz, set_hz, 0);
    props!(amplitude, set_amplitude, 1);
    props!(arg, set_arg, 2);
}

impl Signal for Oscillator {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, sample_rate: Real) {
        let phase = outputs.value(self.phase).expect("phase must be In");
        let hz = self.hz(controls, outputs);
        let amp = self.amplitude(controls, outputs);
        let arg = self.arg(controls, outputs);
        match self.phase {
            Control::V(In::Fix(p)) => {
                let mut ph = p + hz / sample_rate;
                while ph >= 1.0 {
                    ph -= 1.0
                }
                while ph <= -1.0 {
                    ph += 1.0
                }
                self.phase = Control::V(In::Fix(ph));
            }
            _ => {}
        };
        outputs[(self.tag, 0)] = amp * (self.signal_fn)(phase, arg);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ConstBuilder {
    value: Real,
}

/// An synth module that returns a constant Control value. Useful for example to
/// multiply or add constants to oscillators.
#[derive(Debug, Copy, Clone)]
pub struct Const {
    tag: Tag,
    value: Real,
}

impl ConstBuilder {
    pub fn new(value: Real) -> Self {
        Self { value }
    }
    pub fn rack(&self, rack: &mut Rack, _controls: &mut Controls) -> Box<Const> {
        let tag = rack.num_modules();
        let out = Box::new(Const::new(tag, self.value));
        rack.push(out.clone());
        out
    }
}

impl Const {
    pub fn new(tag: Tag, value: Real) -> Self {
        Self { tag, value }
    }
}

impl Signal for Const {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        outputs[(self.tag, 0)] = self.value;
    }
}

#[derive(Copy, Clone)]
pub enum NoiseDistribution {
    StdNormal,
    Uni,
}

/// White noise oscillator.
#[derive(Copy, Clone)]
pub struct WhiteNoise {
    tag: Tag,
    dist: NoiseDistribution,
}

#[derive(Copy, Clone)]
pub struct WhiteNoiseBuilder {
    amplitude: Control,
    dist: NoiseDistribution,
}

impl WhiteNoiseBuilder {
    pub fn new() -> Self {
        Self {
            amplitude: 1.into(),
            dist: NoiseDistribution::StdNormal,
        }
    }
    pub fn dist(&mut self, arg: NoiseDistribution) -> &mut Self {
        self.dist = arg;
        self
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<WhiteNoise> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.amplitude;
        let noise = Box::new(WhiteNoise::new(tag, self.dist));
        rack.push(noise.clone());
        noise
    }
}

impl WhiteNoise {
    pub fn new(tag: Tag, dist: NoiseDistribution) -> Self {
        Self { tag, dist }
    }
    props!(amplitude, set_amplitude, 0);
}

impl Signal for WhiteNoise {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let amplitude = self.amplitude(controls, outputs);
        let mut rng = thread_rng();
        let out: Real;
        match self.dist {
            NoiseDistribution::Uni => {
                out = amplitude * Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng)
            }
            NoiseDistribution::StdNormal => out = amplitude * rng.sample::<Real, _>(StandardNormal),
        }
        outputs[(self.tag, 0)] = out;
    }
}

#[derive(Copy, Clone)]
pub struct PinkNoise {
    tag: Tag,
    b: [Real; 7],
}

#[derive(Copy, Clone)]
pub struct PinkNoiseBuilder {
    amplitude: Control,
}

impl PinkNoise {
    pub fn new(tag: Tag) -> Self {
        Self { tag, b: [0.0; 7] }
    }
    props!(amplitude, set_amplitude, 0);
}

impl PinkNoiseBuilder {
    pub fn new() -> Self {
        Self {
            amplitude: 1.into(),
        }
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<PinkNoise> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.amplitude;
        let noise = Box::new(PinkNoise::new(tag));
        rack.push(noise.clone());
        noise
    }
}

impl Signal for PinkNoise {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let amplitude = self.amplitude(controls, outputs);
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
        outputs[(self.tag, 0)] = pink * amplitude;
    }
}

#[derive(Clone)]
pub struct FourierOsc {
    tag: Tag,
    phases: Vec<Real>,
    coefficients: Vec<Real>,
    lanczos: bool,
}

#[derive(Clone)]
pub struct FourierOscBuilder {
    hz: Control,
    amplitude: Control,
    coefficients: Vec<Real>,
    lanczos: bool,
}

impl FourierOsc {
    pub fn new(tag: Tag, coefficients: Vec<Real>, lanczos: bool) -> Self {
        let n = coefficients.len();
        FourierOsc {
            tag,
            phases: vec![0.0; n],
            coefficients,
            lanczos,
        }
    }
    props!(hz, set_hz, 0);
    props!(amplitude, set_amplitude, 1);
    pub fn lanczos(&self) -> bool {
        self.lanczos
    }
    pub fn set_lacnzos(&mut self, value: bool) {
        self.lanczos = value;
    }
}

impl FourierOscBuilder {
    pub fn new(coefficients: Vec<Real>) -> Self {
        Self {
            hz: 0.into(),
            amplitude: 1.into(),
            coefficients,
            lanczos: true,
        }
    }
    build!(hz);
    build!(amplitude);
    pub fn lanczos(&mut self, value: bool) -> &mut Self {
        self.lanczos = value;
        self
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<FourierOsc> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.hz;
        controls[(tag, 1)] = self.amplitude;
        let osc = Box::new(FourierOsc::new(
            tag,
            self.coefficients.clone(),
            self.lanczos,
        ));
        rack.push(osc.clone());
        osc
    }
}

fn sinc(x: Real) -> Real {
    if x == 0.0 {
        return 1.0;
    }
    (consts::PI * x).sin() / (consts::PI * x)
}

impl Signal for FourierOsc {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, sample_rate: Real) {
        let hz = self.hz(controls, outputs);
        let sigma = self.lanczos as i32;
        let mut out = 0.0;
        for (i, c) in self.coefficients.iter().enumerate() {
            out += c
                * sinc(sigma as Real * i as Real / self.coefficients.len() as Real)
                * (self.phases[i] * TAU).sin();
            self.phases[i] += hz * i as Real / sample_rate;
            while self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }
            while self.phases[i] <= -1.0 {
                self.phases[i] += 1.0;
            }
        }
        outputs[(self.tag, 0)] = out * self.amplitude(controls, outputs);
    }
}

pub fn square_wave(n: u32) -> FourierOscBuilder {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as Real);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOscBuilder::new(coefficients)
}

pub fn triangle_wave(n: u32) -> FourierOscBuilder {
    let mut coefficients: Vec<Real> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as Real);
        } else {
            coefficients.push(0.0);
        }
    }
    FourierOscBuilder::new(coefficients)
}

/// A `SynthModule` that emits 1.0 every `interval` seconds otherwise it emits
/// 0.0.
#[derive(Copy, Clone)]
pub struct Clock {
    tag: Tag,
    clock: u64,
}

#[derive(Copy, Clone)]
pub struct ClockBuilder {
    interval: Control,
}

impl ClockBuilder {
    pub fn new<T: Into<Control>>(interval: T) -> Self {
        Self { interval: interval.into() }
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Box<Clock> {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.interval;
        let clock = Box::new(Clock::new(tag));
        rack.push(clock.clone());
        clock
    }
}

impl Clock {
    pub fn new(tag: Tag) -> Self {
        Self {
            tag,
            clock: 0,
        }
    }
    props!(interval, set_interval, 0)
}

impl Signal for Clock {
    tag!();
    fn signal(&mut self, controls: & Controls, outputs: &mut Outputs, sample_rate: Real) {
        let interval = (self.interval(controls, outputs) * sample_rate) as u64;
        let out;
        if self.clock == 0 {
            self.clock += 1;
            out = 1.0;
        } else {
            self.clock += 1;
            while self.clock >= interval {
                self.clock -= interval;
            }
            out = 0.0;
        }
        outputs[(self.tag, 0)] = out;
    }
}
