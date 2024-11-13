use crate::rack::*;
use crate::utils::{arc_mutex, ArcMutex};
use crate::{build, props, tag};
use arrayvec::ArrayVec;
use math::round::floor;
use rand::prelude::*;
use rand_distr::{StandardNormal, Uniform};
use std::f32::consts;
use std::sync::{Arc, Mutex};

const TAU: f32 = 2.0 * consts::PI;
const MAX_FOURIER_COEFFICIENTS: usize = 64;

pub struct OscBuilder {
    signal_fn: SignalFn,
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
    signal_fn: SignalFn,
    phase: f32,
}

impl OscBuilder {
    pub fn new(signal_fn: SignalFn) -> Self {
        Self {
            signal_fn,
            phase: 0.0,
            hz: 0.0.into(),
            amplitude: 1.0.into(),
            arg: 0.5.into(),
        }
    }

    build!(hz);
    build!(amplitude);
    build!(arg);

    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Oscillator> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.hz;
        rack.controls[(n, 1)] = self.amplitude;
        rack.controls[(n, 2)] = self.arg;
        let osc = Arc::new(Mutex::new(Oscillator::new(n, self.signal_fn, self.phase)));
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
    pub fn new<T: Into<Tag>>(tag: T, signal_fn: SignalFn, phase: f32) -> Self {
        Self {
            tag: tag.into(),
            signal_fn,
            phase,
        }
    }
    props!(hz, set_hz, 0);
    props!(amplitude, set_amplitude, 1);
    props!(arg, set_arg, 2);
}

impl Signal for Oscillator {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let hz = self.hz(rack);
        let amp = self.amplitude(rack);
        let arg = self.arg(rack);
        let mut ph = self.phase + hz / sample_rate;
        while ph >= 1.0 {
            ph -= 1.0
        }
        while ph <= -1.0 {
            ph += 1.0
        }
        self.phase = ph;
        rack.outputs[(self.tag, 0)] = amp * (self.signal_fn)(self.phase, arg);
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
    pub fn rack(&self, rack: &mut Rack) -> Arc<Mutex<Const>> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.value;
        let out = arc_mutex(Const::new(n));
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<WhiteNoise> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.amplitude;
        let noise = arc_mutex(WhiteNoise::new(n, self.dist));
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
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
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
    whites: [f32; 7],
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
        Self {
            tag: tag.into(),
            whites: [0.0; 7],
        }
    }
    props!(amplitude, set_amplitude, 0);
}

impl PinkNoiseBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<PinkNoise> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.amplitude;
        let noise = arc_mutex(PinkNoise::new(n));
        rack.push(noise.clone());
        noise
    }
}

impl Signal for PinkNoise {
    tag!();
    fn signal(&mut self, rack: &mut Rack, _sample_rate: f32) {
        let amplitude = self.amplitude(rack);
        let mut rng = thread_rng();
        let white = Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng);
        self.whites[0] = 0.99886 * self.whites[0] + white * 0.0555179;
        self.whites[1] = 0.99332 * self.whites[1] + white * 0.0750759;
        self.whites[2] = 0.969 * self.whites[2] + white * 0.153852;
        self.whites[3] = 0.8665 * self.whites[3] + white * 0.3104856;
        self.whites[4] = 0.55 * self.whites[4] + white * 0.5329522;
        self.whites[5] = -0.7616 * self.whites[5] - white * 0.016898;
        let pink = self.whites[0]
            + self.whites[1]
            + self.whites[2]
            + self.whites[3]
            + self.whites[4]
            + self.whites[5]
            + self.whites[6]
            + white * 0.5362;
        self.whites[6] = white * 0.115926;
        rack.outputs[(self.tag, 0)] = pink * amplitude;
    }
}

#[derive(Clone)]
pub struct FourierOsc {
    tag: Tag,
    coefficients: ArrayVec<f32, 64>,
    lanczos: bool,
}

#[derive(Clone)]
pub struct FourierOscBuilder {
    hz: Control,
    amplitude: Control,
    coefficients: ArrayVec<f32, 64>,
    lanczos: bool,
}

impl FourierOsc {
    pub fn new<T: Into<Tag>>(tag: T, coefficients: ArrayVec<f32, 64>, lanczos: bool) -> Self {
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
    pub fn new(coefficients: ArrayVec<f32, 64>) -> Self {
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<FourierOsc> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.hz;
        rack.controls[(n, 1)] = self.amplitude;
        let osc = arc_mutex(FourierOsc::new(n, self.coefficients.clone(), self.lanczos));
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
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let hz = self.hz(rack);
        let sigma = self.lanczos as i32;
        let mut out = 0.0;
        let n = self.coefficients.len();
        for (i, c) in self.coefficients.iter_mut().enumerate() {
            out += *c * sinc(sigma as f32 * i as f32 / n as f32) * (*c * TAU).sin();
            *c += hz * i as f32 / sample_rate;
            while *c >= 1.0 {
                *c -= 1.0;
            }
            while *c <= -1.0 {
                *c += 1.0;
            }
        }
        rack.outputs[(self.tag, 0)] = out * self.amplitude(rack);
    }
}

pub fn square_wave(n: u32) -> FourierOscBuilder {
    let mut coefficients: ArrayVec<f32, MAX_FOURIER_COEFFICIENTS> = ArrayVec::new();
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
    let mut coefficients: ArrayVec<f32, MAX_FOURIER_COEFFICIENTS> = ArrayVec::new();
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
    tick: f32,
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Clock> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.interval;
        let clock = arc_mutex(Clock::new(n));
        rack.push(clock.clone());
        clock
    }
}

impl Clock {
    pub fn new<T: Into<Tag>>(tag: T) -> Self {
        Self {
            tag: tag.into(),
            tick: 0.0,
        }
    }
    props!(interval, set_interval, 0);
}

impl Signal for Clock {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let interval = self.interval(rack) * sample_rate;
        let out = if self.tick == 0.0 {
            self.tick += 1.0;
            1.0
        } else {
            self.tick += 1.0;
            while self.tick >= interval {
                self.tick -= interval;
            }
            0.0
        };
        rack.outputs[(self.tag, 0)] = out;
    }
}
