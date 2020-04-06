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

impl SimpleWave for SineWave {
    fn hz(&self) -> f64 {
        self.0.hz
    }
    fn set_hz(&mut self, hz: f64) {
        self.0.hz = hz;
    }
    fn amplitude(&self) -> f32 {
        self.0.amplitude
    }
    fn set_amplitude(&mut self, amp: f32) {
        self.0.amplitude = amp;
    }
    fn phase(&self) -> f64 {
        self.0.phase
    }
    fn set_phase(&mut self, phase: f64) {
        self.0.phase = phase;
    }
    fn hz0(&self) -> f64 {
        self.0.hz0
    }
    fn set_hz0(&mut self, hz0: f64) {
        self.0.hz0 = hz0;
    }
}

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

impl SimpleWave for SquareWave {
    fn hz(&self) -> f64 {
        self.0.hz
    }
    fn set_hz(&mut self, hz: f64) {
        self.0.hz = hz;
    }
    fn amplitude(&self) -> f32 {
        self.0.amplitude
    }
    fn set_amplitude(&mut self, amp: f32) {
        self.0.amplitude = amp;
    }
    fn phase(&self) -> f64 {
        self.0.phase
    }
    fn set_phase(&mut self, phase: f64) {
        self.0.phase = phase;
    }
    fn hz0(&self) -> f64 {
        self.0.hz0
    }
    fn set_hz0(&mut self, hz0: f64) {
        self.0.hz0 = hz0;
    }
}

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
    2. * saw_amp.abs() - wave.0.amplitude
});

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
        wave_params: WaveParams,
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
    }
);

impl ADSRWave {
    fn adsr(&self, t: f32) -> f32 {
        let a = self.attack * TAU;
        let d = self.decay * TAU;
        let s = self.sustain_time * TAU;
        let r = self.release * TAU;
        let sl = self.sustain_level;
        let t = t % TAU;
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
        self.wave_params.amplitude * self.adsr(TAU * self.wave_params.phase as f32)
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave_params.update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave_params.mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave_params.mod_hz(factor);
    }
}

pub_struct!(struct WeightedWave(Box<dyn Wave + Send>, f32));

impl fmt::Debug for WeightedWave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("").field("weight", &self.1).finish()
    }
}
pub_struct!(
    #[derive(Debug)]
    struct AvgWave {
        waves: Vec<WeightedWave>,
    }
);

impl AvgWave {
    pub fn set_weights(&mut self, weights: Vec<f32>) {
        for (i, v) in self.waves.iter_mut().enumerate() {
            v.1 = weights[i];
        }
    }
}

impl Wave for AvgWave {
    fn sample(&self) -> f32 {
        let total_weight = self.waves.iter().fold(0.0, |acc, x| acc + x.1);
        self.waves
            .iter()
            .fold(0.0, |acc, x| acc + x.1 * x.0.sample())
            / total_weight
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
