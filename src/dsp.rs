use math::round::floor;
use std::{
    f64::consts::PI,
    sync::{Arc, Mutex},
};

pub const TAU64: f64 = 2.0 * PI;
pub const TAU32: f32 = TAU64 as f32;

pub type Phase = f64;
pub type Hz = f64;
pub type Amp = f32;

pub trait Wave {
    fn sample(&self) -> Amp;
    fn update_phase(&mut self, add: Phase, sample_rate: f64);
}

pub type ArcWave = Arc<Mutex<dyn Wave + Send>>;
pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

#[derive(Clone)]
pub struct WaveParams {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
}

impl WaveParams {
    fn new(hz: f64) -> Self {
        WaveParams {
            hz,
            amplitude: 1.0,
            phase: 0.0,
        }
    }

    fn update_phase(&mut self, add: Phase, sample_rate: f64) {
        self.phase += (self.hz + add * self.hz) / sample_rate;
        self.phase %= sample_rate;
    }
}

basic_wave!(SineWave, |wave: &SineWave| {
    wave.0.amplitude * (TAU32 * wave.0.phase as f32).sin()
});

basic_wave!(SquareWave, |wave: &SquareWave| {
    let amp = wave.0.amplitude;
    let t = wave.0.phase - floor(wave.0.phase, 0);
    if t < 0.001 {
        return 0.;
    }; // Solely to make work in oscilloscope
    if t <= 0.5 {
        amp
    } else {
        -amp
    }
});

basic_wave!(RampWave, |wave: &RampWave| {
    wave.0.amplitude * (2. * (wave.0.phase - floor(0.5 + wave.0.phase, 0))) as f32
});

basic_wave!(SawWave, |wave: &SawWave| {
    let t = wave.0.phase - 0.5;
    let s = -t - floor(0.5 - t, 0);
    if s < -0.499 {
        return 0.;
    }; // Solely to make work in oscilloscope
    wave.0.amplitude * 2. * s as f32
});

basic_wave!(TriangleWave, |wave: &TriangleWave| {
    let t = wave.0.phase - 0.75;
    let saw_amp = (2. * (-t - floor(0.5 - t, 0))) as f32;
    (2. * saw_amp.abs() - wave.0.amplitude) * wave.0.amplitude
});

// pub struct FourierWave(pub PolyWave);
pub struct FourierWave {
    pub hz: Hz,
    pub amplitude: Amp,
    pub phase: Phase,
    pub sines: Vec<SineWave>,
}

impl FourierWave {
    pub fn new(coefficients: &[f32], hz: Hz) -> Self {
        let mut wwaves: Vec<SineWave> = Vec::new();
        for (n, c) in coefficients.iter().enumerate() {
            let wp = WaveParams {
                hz: hz * n as f64,
                amplitude: *c,
                phase: 0.,
            };
            let s = SineWave(wp);
            wwaves.push(s);
        }
        FourierWave {
            hz,
            amplitude: 1.0,
            phase: 0.0,
            sines: wwaves,
        }
    }

    pub fn boxed(coefficients: &[Amp], hz: Hz) -> ArcMutex<Self> {
        arc(FourierWave::new(coefficients, hz))
    }

    pub fn set_hz(&mut self, hz: Hz) {
        self.hz = hz;
        for n in 0..self.sines.len() {
            self.sines[n].0.hz = hz * n as f64;
        }
    }
}

impl Wave for FourierWave {
    fn sample(&self) -> Amp {
        self.amplitude * self.sines.iter().fold(0., |acc, x| acc + x.sample())
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        for w in self.sines.iter_mut() {
            w.update_phase(0.0, sample_rate);
        }
    }
}

pub fn square_wave(n: u32, hz: Hz) -> ArcMutex<FourierWave> {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierWave::boxed(coefficients.as_ref(), hz)
}

pub fn triangle_wave(n: u32, hz: Hz) -> ArcMutex<FourierWave> {
    let mut coefficients: Vec<Amp> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierWave::boxed(coefficients.as_ref(), hz)
}
