use crate::rack::*;
use crate::{build, props, tag};
use math::round::floor;
use rand::prelude::*;
use rand_distr::{StandardNormal, Uniform};
use std::f32::consts;
use std::sync::Arc;

const TAU: f32 = 2.0 * consts::PI;

pub struct OscBuilder {
    signal_fn: fn(f32, f32) -> f32,
    phase: f32,
    hz: Control,
    amplitude: Control,
    arg: Control,
}

/// A standard oscillator that has phase, hz, and amp. Pass in a signal function
/// to operate on the phase and an optional extra argument.
#[derive(Clone)]
pub struct Oscillator {
    tag: Tag,
    signal_fn: fn(f32, f32) -> f32,
}

impl OscBuilder {
    pub fn new(signal_fn: fn(f32, f32) -> f32) -> Self {
        Self {
            signal_fn,
            phase: 0.0,
            hz: 0.0.into(),
            amplitude: 1.0.into(),
            arg: 0.5.into(),
        }
    }

    pub fn phase(&mut self, value: f32) -> &mut Self {
        self.phase = value;
        self
    }

    build!(hz);
    build!(amplitude);
    build!(arg);

    pub fn rack(&self, rack: &mut Rack) -> Arc<Oscillator> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.hz;
        rack.controls[(n, 1)] = self.amplitude;
        rack.controls[(n, 2)] = self.arg;
        rack.state[(n, 0)] = self.phase;
        let osc = Arc::new(Oscillator::new(n, self.signal_fn));
        rack.push(osc.clone());
        osc
    }
}

pub fn sine_osc(phase: f32, _: f32) -> f32 {
    (phase * TAU).sin()
}

pub fn square_osc(phase: f32, duty_cycle: f32) -> f32 {
    let t = phase - phase.floor();
    if t <= duty_cycle {
        1.0
    } else {
        -1.0
    }
}

pub fn saw_osc(phase: f32, _: f32) -> f32 {
    let t = phase - 0.5;
    let s = -t - floor(0.5 - t as f64, 0) as f32;
    if s < -0.5 {
        0.0
    } else {
        2.0 * s
    }
}

pub fn triangle_osc(phase: f32, _: f32) -> f32 {
    let t = phase - 0.75;
    let saw_amp = 2. * (-t - floor(0.5 - t as f64, 0) as f32);
    2.0 * saw_amp.abs() - 1.0
}

impl Oscillator {
    pub fn new<T: Into<Tag>>(tag: T, signal_fn: fn(f32, f32) -> f32) -> Self {
        Self {
            tag: tag.into(),
            signal_fn,
        }
    }
    pub fn phase(&self, state: &State) -> f32 {
        state[(self.tag, 0)]
    }
    pub fn set_phase(&self, state: &mut State, value: f32) {
        state[(self.tag, 0)] = value;
    }
    props!(hz, set_hz, 0);
    props!(amplitude, set_amplitude, 1);
    props!(arg, set_arg, 2);
}

impl Signal for Oscillator {
    tag!();
    fn signal(&self, rack: &mut Rack, sample_rate: f32) {
        let phase = self.phase(&rack.state);
        let hz = self.hz(rack);
        let amp = self.amplitude(rack);
        let arg = self.arg(rack);
        let mut ph = phase + hz / sample_rate;
        while ph >= 1.0 {
            ph -= 1.0
        }
        while ph <= -1.0 {
            ph += 1.0
        }
        self.set_phase(&mut rack.state, ph);
        rack.outputs[(self.tag, 0)] = amp * (self.signal_fn)(phase, arg);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ConstBuilder {
    value: Control,
}

/// An synth module that returns a constant Control value. Useful for example to
/// multiply or add constants to oscillators.
#[derive(Debug, Copy, Clone)]
pub struct Const {
    tag: Tag,
}

impl ConstBuilder {
    pub fn new(value: Control) -> Self {
        Self { value }
    }
    pub fn rack(&self, rack: &mut Rack) -> Arc<Const> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.value;
        let out = Arc::new(Const::new(n));
        rack.push(out.clone());
        out
    }
}

impl Const {
    pub fn new<T: Into<Tag>>(tag: T) -> Self {
        Self { tag: tag.into() }
    }
    props!(value, set_value, 0);
}

impl Signal for Const {
    tag!();
    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = self.value(rack);
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

impl Default for WhiteNoiseBuilder {
    fn default() -> Self {
        Self {
            amplitude: 1.0.into(),
            dist: NoiseDistribution::StdNormal,
        }
    }
}

impl WhiteNoiseBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn dist(&mut self, arg: NoiseDistribution) -> &mut Self {
        self.dist = arg;
        self
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack) -> Arc<WhiteNoise> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.amplitude;
        let noise = Arc::new(WhiteNoise::new(n, self.dist));
        rack.push(noise.clone());
        noise
    }
}

impl WhiteNoise {
    pub fn new<T: Into<Tag>>(tag: T, dist: NoiseDistribution) -> Self {
        Self {
            tag: tag.into(),
            dist,
        }
    }
    props!(amplitude, set_amplitude, 0);
}

impl Signal for WhiteNoise {
    tag!();
    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        let amplitude = self.amplitude(rack);
        let mut rng = thread_rng();
        let out = match self.dist {
            NoiseDistribution::Uni => {
                amplitude * Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng)
            }
            NoiseDistribution::StdNormal => amplitude * rng.sample::<f32, _>(StandardNormal),
        };
        rack.outputs[(self.tag, 0)] = out;
    }
}

#[derive(Copy, Clone)]
pub struct PinkNoise {
    tag: Tag,
}

#[derive(Copy, Clone)]
pub struct PinkNoiseBuilder {
    amplitude: Control,
}

impl Default for PinkNoiseBuilder {
    fn default() -> Self {
        Self {
            amplitude: 1.0.into(),
        }
    }
}

impl PinkNoise {
    pub fn new<T: Into<Tag>>(tag: T) -> Self {
        Self { tag: tag.into() }
    }
    props!(amplitude, set_amplitude, 0);
}

impl PinkNoiseBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack) -> Arc<PinkNoise> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.amplitude;
        let noise = Arc::new(PinkNoise::new(n));
        rack.push(noise.clone());
        noise
    }
}

impl Signal for PinkNoise {
    tag!();
    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        let tag = self.tag;
        let amplitude = self.amplitude(rack);
        let mut rng = thread_rng();
        let white = Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng);
        rack.state[(tag, 0)] = 0.99886 * rack.state[(tag, 0)] + white * 0.0555179;
        rack.state[(tag, 1)] = 0.99332 * rack.state[(tag, 1)] + white * 0.0750759;
        rack.state[(tag, 2)] = 0.969 * rack.state[(tag, 2)] + white * 0.153852;
        rack.state[(tag, 3)] = 0.8665 * rack.state[(tag, 3)] + white * 0.3104856;
        rack.state[(tag, 4)] = 0.55 * rack.state[(tag, 4)] + white * 0.5329522;
        rack.state[(tag, 5)] = -0.7616 * rack.state[(tag, 5)] - white * 0.016898;
        let pink = rack.state[(tag, 0)]
            + rack.state[(tag, 1)]
            + rack.state[(tag, 2)]
            + rack.state[(tag, 3)]
            + rack.state[(tag, 4)]
            + rack.state[(tag, 5)]
            + rack.state[(tag, 6)]
            + white * 0.5362;
        rack.state[(tag, 6)] = white * 0.115926;
        rack.outputs[(self.tag, 0)] = pink * amplitude;
    }
}

#[derive(Clone)]
pub struct FourierOsc {
    tag: Tag,
    coefficients: Vec<f32>,
    lanczos: bool,
}

#[derive(Clone)]
pub struct FourierOscBuilder {
    hz: Control,
    amplitude: Control,
    coefficients: Vec<f32>,
    lanczos: bool,
}

impl FourierOsc {
    pub fn new<T: Into<Tag>>(tag: T, coefficients: Vec<f32>, lanczos: bool) -> Self {
        assert!(coefficients.len() <= 64, "Max size of fourier osc is 64");
        FourierOsc {
            tag: tag.into(),
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
    pub fn new(coefficients: Vec<f32>) -> Self {
        Self {
            hz: 0.0.into(),
            amplitude: 1.0.into(),
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
    pub fn rack(&self, rack: &mut Rack) -> Arc<FourierOsc> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.hz;
        rack.controls[(n, 1)] = self.amplitude;
        let osc = Arc::new(FourierOsc::new(n, self.coefficients.clone(), self.lanczos));
        rack.push(osc.clone());
        osc
    }
}

fn sinc(x: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    (consts::PI * x).sin() / (consts::PI * x)
}

impl Signal for FourierOsc {
    tag!();
    fn signal(&self, rack: &mut Rack, sample_rate: f32) {
        let tag = self.tag;
        let hz = self.hz(rack);
        let sigma = self.lanczos as i32;
        let mut out = 0.0;
        for (i, c) in self.coefficients.iter().enumerate() {
            out += c
                * sinc(sigma as f32 * i as f32 / self.coefficients.len() as f32)
                * (rack.state[(tag, i)] * TAU).sin();
            rack.state[(tag, i)] += hz * i as f32 / sample_rate;
            while rack.state[(tag, i)] >= 1.0 {
                rack.state[(tag, i)] -= 1.0;
            }
            while rack.state[(tag, i)] <= -1.0 {
                rack.state[(tag, i)] += 1.0;
            }
        }
        rack.outputs[(self.tag, 0)] = out * self.amplitude(rack);
    }
}

pub fn square_wave(n: u32) -> FourierOscBuilder {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOscBuilder::new(coefficients)
}

pub fn triangle_wave(n: u32) -> FourierOscBuilder {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as f32);
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
}

#[derive(Copy, Clone)]
pub struct ClockBuilder {
    interval: Control,
}

impl ClockBuilder {
    pub fn new<T: Into<Control>>(interval: T) -> Self {
        Self {
            interval: interval.into(),
        }
    }
    pub fn rack(&self, rack: &mut Rack) -> Arc<Clock> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.interval;
        let clock = Arc::new(Clock::new(n));
        rack.push(clock.clone());
        clock
    }
}

impl Clock {
    pub fn new<T: Into<Tag>>(tag: T) -> Self {
        Self { tag: tag.into() }
    }
    props!(interval, set_interval, 0);
}

impl Signal for Clock {
    tag!();
    fn signal(&self, rack: &mut Rack, sample_rate: f32) {
        let tag = self.tag;
        let interval = self.interval(rack) * sample_rate;
        let out = if rack.state[(tag, 0)] == 0.0 {
            rack.state[(tag, 0)] += 1.0;
            1.0
        } else {
            rack.state[(tag, 0)] += 1.0;
            while rack.state[(tag, 0)] >= interval {
                rack.state[(tag, 0)] -= interval;
            }
            0.0
        };
        rack.outputs[(self.tag, 0)] = out;
    }
}
