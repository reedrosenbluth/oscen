use math::round::floor;
use rand::distributions::Uniform;
use rand::prelude::*;
use std::{
    f64::consts::PI,
    sync::{Arc, Mutex, MutexGuard},
};

pub const TAU64: f64 = 2.0 * PI;
pub const TAU32: f32 = TAU64 as f32;

pub type Phase = f64;
pub type Hz = f64;
pub type Amp = f32;

pub trait Signal {
    fn signal(&mut self, sample_rate: f64) -> Amp;
}

pub type ArcWave = Arc<Mutex<dyn Signal + Send>>;
pub type ArcMutex<T> = Arc<Mutex<T>>;

pub trait Mtx<T> {
    fn mtx(&self) -> MutexGuard<'_, T>;
}

impl<T> Mtx<T> for ArcMutex<T> {
    fn mtx(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap()
    }
}

pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

pub trait HasHz {
    fn hz(&self) -> Hz;
    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz);

    fn set_hz(&mut self, hz: Hz) {
        self.modify_hz(&|_| hz);
    }
}

pub struct WhiteNoise {
    pub amplitude: f32,
    dist: Uniform<f32>,
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self {
            amplitude: 1.0,
            dist: Uniform::new_inclusive(-1.0, 1.0),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for WhiteNoise {
    fn signal(&mut self, _sample_rate: f64) -> Amp {
        let mut rng = rand::thread_rng();
        self.dist.sample(&mut rng) * self.amplitude
    }
}

#[derive(Clone)]
pub struct SineOsc {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
}

impl SineOsc {
    pub fn new(hz: Hz) -> Self {
        Self {
            hz,
            amplitude: 1.0,
            phase: 0.0,
        }
    }

    pub fn wrapped(hz: Hz) -> ArcMutex<Self> {
        arc(Self::new(hz))
    }
}

impl Signal for SineOsc {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let amp = self.amplitude * (TAU32 * self.phase as f32).sin();
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
        amp
    }
}

impl HasHz for SineOsc {
    fn hz(&self) -> Hz {
        self.hz
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.hz = f(self.hz);
    }
}

#[derive(Clone)]
pub struct SquareOsc {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
}

impl SquareOsc {
    pub fn new(hz: Hz) -> Self {
        Self {
            hz,
            amplitude: 1.0,
            phase: 0.0,
        }
    }

    pub fn wrapped(hz: Hz) -> ArcMutex<Self> {
        arc(Self::new(hz))
    }
}

impl Signal for SquareOsc {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let t = self.phase - floor(self.phase, 0);
        let amp = if t < 0.001 {
            0.0
        } else if t <= 0.5 {
            self.amplitude
        } else {
            -self.amplitude
        };
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
        amp
    }
}

impl HasHz for SquareOsc {
    fn hz(&self) -> Hz {
        self.hz
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.hz = f(self.hz);
    }
}

#[derive(Clone)]
pub struct SawOsc {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
}

impl SawOsc {
    pub fn new(hz: Hz) -> Self {
        Self {
            hz,
            amplitude: 1.0,
            phase: 0.0,
        }
    }

    pub fn wrapped(hz: Hz) -> ArcMutex<Self> {
        arc(Self::new(hz))
    }
}

impl Signal for SawOsc {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let t = self.phase - 0.5;
        let s = -t - floor(0.5 - t, 0);
        let amp = if s < -0.499 {
            0.0
        } else {
            self.amplitude * 2. * s as f32
        };
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
        amp
    }
}

impl HasHz for SawOsc {
    fn hz(&self) -> Hz {
        self.hz
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.hz = f(self.hz);
    }
}

#[derive(Clone)]
pub struct TriangleOsc {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
}

impl TriangleOsc {
    pub fn new(hz: Hz) -> Self {
        Self {
            hz,
            amplitude: 1.0,
            phase: 0.0,
        }
    }

    pub fn wrapped(hz: Hz) -> ArcMutex<Self> {
        arc(Self::new(hz))
    }
}

impl Signal for TriangleOsc {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let t = self.phase - 0.75;
        let saw_amp = (2. * (-t - floor(0.5 - t, 0))) as f32;
        let amp = (2. * saw_amp.abs() - self.amplitude) * self.amplitude;
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
        amp
    }
}

impl HasHz for TriangleOsc {
    fn hz(&self) -> Hz {
        self.hz
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.hz = f(self.hz);
    }
}

// pub struct FourierWave(pub PolyWave);
pub struct FourierOsc {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
    pub sines: Vec<SineOsc>,
}

impl FourierOsc {
    pub fn new(coefficients: &[f32], hz: Hz) -> Self {
        let mut wwaves: Vec<SineOsc> = Vec::new();
        for (n, c) in coefficients.iter().enumerate() {
            let mut s = SineOsc::new(hz * n as f64);
            s.amplitude = *c;
            wwaves.push(s);
        }
        FourierOsc {
            hz,
            amplitude: 1.0,
            phase: 0.0,
            sines: wwaves,
        }
    }

    pub fn wrapped(coefficients: &[Amp], hz: Hz) -> ArcMutex<Self> {
        arc(FourierOsc::new(coefficients, hz))
    }
}

impl Signal for FourierOsc {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        self.amplitude
            * self
                .sines
                .iter_mut()
                .fold(0., |acc, x| acc + x.signal(sample_rate))
    }
}

impl HasHz for FourierOsc {
    fn hz(&self) -> Hz {
        self.hz
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.hz = f(self.hz);
        for n in 0..self.sines.len() {
            self.sines[n].set_hz(self.hz * n as f64);
        }
    }
}

pub fn square_wave(n: u32, hz: Hz) -> ArcMutex<FourierOsc> {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::wrapped(coefficients.as_ref(), hz)
}

pub fn triangle_wave(n: u32, hz: Hz) -> ArcMutex<FourierOsc> {
    let mut coefficients: Vec<Amp> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierOsc::wrapped(coefficients.as_ref(), hz)
}
