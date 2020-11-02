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

    pub fn rack(
        &self,
        rack: &mut Rack,
        controls: &mut Controls,
        state: &mut State,
    ) -> Arc<Oscillator> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.hz;
        controls[(n, 1)] = self.amplitude;
        controls[(n, 2)] = self.arg;
        state[(n, 0)] = self.phase;
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
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let phase = self.phase(state);
        let hz = self.hz(controls, outputs);
        let amp = self.amplitude(controls, outputs);
        let arg = self.arg(controls, outputs);
        let mut ph = phase + hz / sample_rate;
        while ph >= 1.0 {
            ph -= 1.0
        }
        while ph <= -1.0 {
            ph += 1.0
        }
        self.set_phase(state, ph);
        outputs[(self.tag, 0)] = amp * (self.signal_fn)(phase, arg);
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Const> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.value;
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
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        outputs[(self.tag, 0)] = self.value(controls, outputs);
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
            amplitude: 1.0.into(),
            dist: NoiseDistribution::StdNormal,
        }
    }
    pub fn dist(&mut self, arg: NoiseDistribution) -> &mut Self {
        self.dist = arg;
        self
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<WhiteNoise> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.amplitude;
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
    fn signal(
        &self,
        controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let amplitude = self.amplitude(controls, outputs);
        let mut rng = thread_rng();
        let out: f32;
        match self.dist {
            NoiseDistribution::Uni => {
                out = amplitude * Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng)
            }
            NoiseDistribution::StdNormal => out = amplitude * rng.sample::<f32, _>(StandardNormal),
        }
        outputs[(self.tag, 0)] = out;
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

impl PinkNoise {
    pub fn new<T: Into<Tag>>(tag: T) -> Self {
        Self { tag: tag.into() }
    }
    props!(amplitude, set_amplitude, 0);
}

impl PinkNoiseBuilder {
    pub fn new() -> Self {
        Self {
            amplitude: 0.25.into(),
        }
    }
    build!(amplitude);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<PinkNoise> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.amplitude;
        let noise = Arc::new(PinkNoise::new(n));
        rack.push(noise.clone());
        noise
    }
}

impl Signal for PinkNoise {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        let tag = self.tag;
        let amplitude = self.amplitude(controls, outputs);
        let mut rng = thread_rng();
        let white = Uniform::new_inclusive(-1.0, 1.0).sample(&mut rng);
        state[(tag, 0)] = 0.99886 * state[(tag, 0)] + white * 0.0555179;
        state[(tag, 1)] = 0.99332 * state[(tag, 1)] + white * 0.0750759;
        state[(tag, 2)] = 0.96900 * state[(tag, 2)] + white * 0.1538520;
        state[(tag, 3)] = 0.86650 * state[(tag, 3)] + white * 0.3104856;
        state[(tag, 4)] = 0.55000 * state[(tag, 4)] + white * 0.5329522;
        state[(tag, 5)] = -0.7616 * state[(tag, 5)] - white * 0.0168980;
        let pink = state[(tag, 0)]
            + state[(tag, 1)]
            + state[(tag, 2)]
            + state[(tag, 3)]
            + state[(tag, 4)]
            + state[(tag, 5)]
            + state[(tag, 6)]
            + white * 0.5362;
        state[(tag, 6)] = white * 0.115926;
        outputs[(self.tag, 0)] = pink * amplitude;
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<FourierOsc> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.hz;
        controls[(n, 1)] = self.amplitude;
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
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let tag = self.tag;
        let hz = self.hz(controls, outputs);
        let sigma = self.lanczos as i32;
        let mut out = 0.0;
        for (i, c) in self.coefficients.iter().enumerate() {
            out += c
                * sinc(sigma as f32 * i as f32 / self.coefficients.len() as f32)
                * (state[(tag, i)] * TAU).sin();
            state[(tag, i)] += hz * i as f32 / sample_rate;
            while state[(tag, i)] >= 1.0 {
                state[(tag, i)] -= 1.0;
            }
            while state[(tag, i)] <= -1.0 {
                state[(tag, i)] += 1.0;
            }
        }
        outputs[(self.tag, 0)] = out * self.amplitude(controls, outputs);
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Clock> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.interval;
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
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let tag = self.tag;
        let interval = self.interval(controls, outputs) * sample_rate;
        let out;
        if state[(tag, 0)] == 0.0 {
            state[(tag, 0)] += 1.0;
            out = 1.0;
        } else {
            state[(tag, 0)] += 1.0;
            while state[(tag, 0)] >= interval {
                state[(tag, 0)] -= interval;
            }
            out = 0.0;
        }
        outputs[(self.tag, 0)] = out;
    }
}
