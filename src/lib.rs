use derive_more::Constructor;
use math::round::floor;
use nannou::prelude::*;
use std::fmt;

mod macros;

pub trait Wave {
    fn sample(&self) -> f32;
    fn update_phase(&mut self, sample_rate: f64);
    fn mul_hz(&mut self, factor: f64);
    fn mod_hz(&mut self, factor: f64);
}

pub trait SimpleWave {
    fn hz(&self) -> f64;
    fn set_hz(&mut self, hz: f64);
    fn amplitude(&self) -> f32;
    fn set_amplitude(&mut self, amp: f32);
    fn phase(&self) -> f64;
    fn set_phase(&mut self, phase: f64);
    fn hz0(&self) -> f64;
    fn set_hz0(&mut self, hz0: f64);
}

pub_struct!(
    #[derive(Clone)]
    struct WaveParams {
        hz: f64,
        amplitude: f32,
        phase: f64,
        hz0: f64,
    }
);

impl WaveParams {
    fn new(hz: f64, amplitude: f32) -> Self {
        WaveParams {
            hz,
            amplitude,
            phase: 0.0,
            hz0: hz,
        }
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
    }

    fn mul_hz(&mut self, factor: f64) {
        self.hz *= factor;
        self.hz0 *= factor;
    }

    fn mod_hz(&mut self, factor: f64) {
        self.hz = self.hz0 * factor;
    }
}

impl SimpleWave for WaveParams {
    fn hz(&self) -> f64 {
        self.hz
    }
    fn set_hz(&mut self, hz: f64) {
        self.hz = hz;
    }
    fn amplitude(&self) -> f32 {
        self.amplitude
    }
    fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp;
    }
    fn phase(&self) -> f64 {
        self.phase
    }
    fn set_phase(&mut self, phase: f64) {
        self.phase = phase;
    }
    fn hz0(&self) -> f64 {
        self.hz0
    }
    fn set_hz0(&mut self, hz0: f64) {
        self.hz0 = hz0;
    }
}

basic_wave!(SineWave, |wave: &SineWave| {
    wave.0.amplitude * (TAU * wave.0.phase as f32).sin()
});

simple_wave!(SineWave);

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

simple_wave!(SquareWave);

basic_wave!(RampWave, |wave: &RampWave| {
    wave.0.amplitude * (2. * (wave.0.phase - floor(0.5 + wave.0.phase, 0))) as f32
});

simple_wave!(RampWave);

basic_wave!(SawWave, |wave: &SawWave| {
    let t = wave.0.phase - 0.5;
    let s = -t - floor(0.5 - t, 0);
    if s < -0.499 {
        return 0.;
    }; // Solely to make work in oscilloscope
    wave.0.amplitude * 2. * s as f32
});

simple_wave!(SawWave);

basic_wave!(TriangleWave, |wave: &TriangleWave| {
    let t = wave.0.phase - 0.75;
    let saw_amp = (2. * (-t - floor(0.5 - t, 0))) as f32;
    2. * saw_amp.abs() - wave.0.amplitude
});

simple_wave!(TriangleWave);

pub_struct!(
    #[derive(Constructor)]
    struct LerpWave {
        wave1: Box<dyn Wave + Send>,
        wave2: Box<dyn Wave + Send>,
        alpha: f32,
    }
);

impl LerpWave {
    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }
}

impl Wave for LerpWave {
    fn sample(&self) -> f32 {
        (1. - self.alpha) * self.wave1.sample() + self.alpha * self.wave2.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.update_phase(sample_rate);
        self.wave2.update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave1.mul_hz(factor);
        self.wave2.mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave1.mod_hz(factor);
        self.wave2.mod_hz(factor);
    }
}

pub_struct!(
    /// Voltage Controlled Amplifier
    struct VCA {
        wave: Box<dyn Wave + Send>,
        cv: Box<dyn Wave + Send>,
    }
);

impl Wave for VCA {
    fn sample(&self) -> f32 {
        self.wave.sample() * self.cv.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.update_phase(sample_rate);
        self.cv.update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave.mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave.mod_hz(factor);
    }
}

pub_struct!(
    /// Voltage Controlled Oscillator
    struct VCO {
        wave: Box<dyn Wave + Send>,
        cv: Box<dyn Wave + Send>,
        fm_mult: i32,
    }
);

impl VCO {
    pub fn fm_mult(&mut self) -> i32 {
        self.fm_mult
    }

    pub fn set_fm_mult(&mut self, mult: i32) {
        self.fm_mult = mult;
    }
}

impl Wave for VCO {
    fn sample(&self) -> f32 {
        self.wave.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.update_phase(sample_rate);
        self.cv.update_phase(sample_rate);

        //Frequency Modulation
        let factor = 2.0.powf(self.cv.sample() * self.fm_mult as f32) as f64;
        self.wave.mod_hz(factor);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave.mul_hz(factor);
        self.cv.mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave.mod_hz(factor);
    }
}

pub_struct!(
    struct ADSRWave {
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
        time: f64,
    }
);

impl ADSRWave {
    fn adsr(&self, t: f32) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let s = self.sustain_time;
        let r = self.release;
        let sl = self.sustain_level;
        match t {
            x if x < a => t / a,
            x if x < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
            x if x < a + d + s => sl,
            x if x < a + d + s + r => sl - (t - a - d - s) * sl / r,
            _ => 0.0,
        }
    }
}

impl Wave for ADSRWave {
    fn sample(&self) -> f32 {
        self.adsr(self.time as f32)
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.time += 1. / sample_rate;
    }

    fn mul_hz(&mut self, _factor: f64) {}
    fn mod_hz(&mut self, _factor: f64) {}
}

pub struct WeightedWave(pub Box<dyn Wave + Send>, pub f32);

impl fmt::Debug for WeightedWave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("").field("weight", &self.1).finish()
    }
}
#[derive(Debug)]
pub struct PolyWave {
    pub waves: Vec<WeightedWave>,
    pub volume: f32,
}

impl PolyWave {
    pub fn new(waves: Vec<WeightedWave>, volume: f32) -> Self {
        Self { waves, volume }
    }

    pub fn new_normal(waves: Vec<WeightedWave>) -> Self {
        let total_weight = waves.iter().fold(0., |acc: f32, x| acc + x.1);
        PolyWave::new(waves, 1. / total_weight)
    }

    pub fn set_weights(&mut self, weights: Vec<f32>) {
        for (i, v) in self.waves.iter_mut().enumerate() {
            v.1 = weights[i];
        }
    }

    pub fn normalize_weights(&mut self) {
        let total_weight = self.waves.iter().fold(0., |acc: f32, x| acc + x.1);
        for w in self.waves.iter_mut() {
            w.1 /= total_weight;
        }
    }
}

impl Wave for PolyWave {
    fn sample(&self) -> f32 {
        self.volume
            * self
                .waves
                .iter()
                .fold(0.0, |acc, x| acc + x.1 * x.0.sample())
    }

    fn update_phase(&mut self, sample_rate: f64) {
        for wave in self.waves.iter_mut() {
            wave.0.update_phase(sample_rate);
        }
    }

    fn mul_hz(&mut self, factor: f64) {
        for wave in self.waves.iter_mut() {
            wave.0.mul_hz(factor);
        }
    }

    fn mod_hz(&mut self, factor: f64) {
        for wave in self.waves.iter_mut() {
            wave.0.mod_hz(factor);
        }
    }
}

pub fn fourier_wave(coefficients: Vec<f32>, hz: f64) -> PolyWave {
    let mut wwaves: Vec<WeightedWave> = Vec::new();
    for (n, c) in coefficients.iter().enumerate() {
        let wp = WaveParams::new(hz * n as f64, 1.0);
        let s = SineWave(wp);
        wwaves.push(WeightedWave(Box::new(s), *c));
    }
    PolyWave::new(wwaves, 1.)
}

pub fn square_wave(n: u32, hz: f64) -> PolyWave {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as f32);
        } else {
            coefficients.push(0.); 
        }
    }
    fourier_wave(coefficients, hz)
}
