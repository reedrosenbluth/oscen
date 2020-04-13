use derive_more::Constructor;
use math::round::floor;
use std::{
    f64::consts::PI,
    rc::Rc,
    sync::{Arc, Mutex},
};

mod macros;

pub const TAU64: f64 = 2.0 * PI;
pub const TAU32: f32 = TAU64 as f32;

pub type Phase = f64;
pub type Hz = f64;
pub type Amp = f32;

pub trait Wave {
    fn sample(&self) -> Amp;
    fn update_phase(&mut self, sample_rate: f64);
    fn mul_hz(&mut self, factor: f64);
    fn mod_hz(&mut self, factor: f64);
    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>);
}

pub type BoxedWave = Box<dyn Wave + Send>;
pub type ArcWave = Arc<Mutex<dyn Wave + Send>>;
pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

pub_struct!(
    #[derive(Clone)]
    struct WaveParams {
        hz: Hz,
        amplitude: Amp,
        phase: Phase,
        hz0: Hz,
    }
);

impl WaveParams {
    fn new(hz: f64) -> Self {
        WaveParams {
            hz,
            amplitude: 1.0,
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

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.amplitude = f(self.amplitude)
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
    2. * saw_amp.abs() - wave.0.amplitude
});

pub struct SumWave<U, W>
where U: Wave + Send,
      W: Wave + Send, 
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<W>,
}

impl<U: Wave + Send, W: Wave + Send> SumWave<U, W> {
    pub fn new(wave1: ArcMutex<U>, wave2: ArcMutex<W>) -> Self { Self { wave1, wave2 } }

    pub fn boxed(wave1: ArcMutex<U>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(SumWave { wave1, wave2 })
    }
}

impl<U: Wave + Send, W: Wave + Send> Wave for SumWave<U, W> {
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        wave1.sample() + wave2.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave1.lock().unwrap().mul_hz(factor);
        self.wave2.lock().unwrap().mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave1.lock().unwrap().mod_hz(factor);
        self.wave2.lock().unwrap().mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.wave1.lock().unwrap().modify_amplitude(f.clone());
        self.wave2.lock().unwrap().modify_amplitude(f);
    }
}
#[derive(Constructor)]
pub struct LerpWave {
    pub wave1: ArcWave,
    pub wave2: ArcWave,
    pub alpha: f32,
}

impl LerpWave {
    pub fn boxed(wave1: ArcWave, wave2: ArcWave, alpha: f32) -> ArcMutex<Self> {
        arc(LerpWave {
            wave1,
            wave2,
            alpha,
        })
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }
}

impl Wave for LerpWave {
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        (1. - self.alpha) * wave1.sample() + self.alpha * wave2.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave1.lock().unwrap().mul_hz(factor);
        self.wave2.lock().unwrap().mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave1.lock().unwrap().mod_hz(factor);
        self.wave2.lock().unwrap().mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.wave1.lock().unwrap().modify_amplitude(f.clone());
        self.wave2.lock().unwrap().modify_amplitude(f);
    }
}

/// Voltage Controlled Amplifier
pub struct VCA {
    pub wave: ArcWave,
    pub cv: ArcWave,
}

impl VCA {
    pub fn boxed(wave: ArcWave, cv: ArcWave) -> ArcMutex<Self> {
        arc(VCA { wave, cv })
    }
}

impl Wave for VCA {
    fn sample(&self) -> f32 {
        self.wave.lock().unwrap().sample() * self.cv.lock().unwrap().sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.cv.lock().unwrap().update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.wave.lock().unwrap().modify_amplitude(f);
    }
}

/// Voltage Controlled Oscillator
pub struct VCO {
    pub wave: ArcWave,
    pub cv: ArcWave,
    pub fm_mult: f64,
}

impl VCO {
    pub fn boxed(wave: ArcWave, cv: ArcWave, fm_mult: f64) -> ArcMutex<Self> {
        arc(VCO { wave, cv, fm_mult })
    }

    pub fn fm_mult(&mut self) -> f64 {
        self.fm_mult
    }

    pub fn set_fm_mult(&mut self, mult: f64) {
        self.fm_mult = mult;
    }
}

impl Wave for VCO {
    fn sample(&self) -> f32 {
        self.wave.lock().unwrap().sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.cv.lock().unwrap().update_phase(sample_rate);

        //Frequency Modulation
        let factor = 2f32.powf(self.cv.lock().unwrap().sample() * self.fm_mult as f32) as f64;
        self.wave.lock().unwrap().mod_hz(factor);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mul_hz(factor);
        self.cv.lock().unwrap().mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.wave.lock().unwrap().modify_amplitude(f);
    }
}

pub struct TriggeredWave {
    pub wave: ArcWave,
    pub attack: f32,
    pub decay: f32,
    pub sustain_level: f32,
    pub release: f32,
    pub clock: f64,
    pub triggered: bool,
}

impl Wave for TriggeredWave {
    fn sample(&self) -> f32 {
        let a = self.attack;
        let d = self.decay;
        let r = self.release;
        let sl = self.sustain_level;
        let level = if self.triggered {
            match self.clock as f32 {
                t if t < a => t / a,
                t if t < a + d => 1.0 + (t - a) * (sl - 1.0) / d,
                _ => sl,
            }
        } else {
            match self.clock as f32 {
                t if t < r => sl - t / r * sl,
                _ => 0.,
            }
        };
        self.wave.lock().unwrap().sample() * level
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave.lock().unwrap().update_phase(sample_rate);
        self.clock += 1. / sample_rate;
    }

    fn mul_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.wave.lock().unwrap().mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.wave.lock().unwrap().modify_amplitude(f);
    }
}

pub_struct!(
    struct ADSRWave {
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
        current_time: f64,
    }
);

impl ADSRWave {
    pub fn new(
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
    ) -> Self {
        ADSRWave {
            attack: attack,
            decay: decay,
            sustain_time: sustain_time,
            sustain_level: sustain_level,
            release: release,
            current_time: 0.,
        }
    }

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
        self.adsr(self.current_time as f32)
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.current_time += 1. / sample_rate;
    }

    fn mul_hz(&mut self, _factor: f64) {}
    fn mod_hz(&mut self, _factor: f64) {}
    fn modify_amplitude(&mut self, _f: Rc<dyn Fn(f32) -> f32>) {}
}

pub struct PolyWave {
    pub waves: Vec<ArcWave>,
    pub volume: f32,
}

impl PolyWave {
    pub fn new(waves: Vec<ArcWave>, volume: f32) -> Self {
        Self { waves, volume }
    }

    pub fn boxed(waves: Vec<ArcWave>, volume: f32) -> ArcMutex<Self> {
        arc(Self::new(waves, volume))
    }

    pub fn set_amplitudes(&mut self, weights: &[f32]) {
        for (i, v) in self.waves.iter_mut().enumerate() {
            let val = weights[i];
            v.lock().unwrap().modify_amplitude(Rc::new(move |_| val));
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
}

impl Wave for PolyWave {
    fn sample(&self) -> f32 {
        self.volume
            * self
                .waves
                .iter()
                .fold(0.0, |acc, x| acc + x.lock().unwrap().sample())
    }

    fn update_phase(&mut self, sample_rate: f64) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().update_phase(sample_rate);
        }
    }

    fn mul_hz(&mut self, factor: f64) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().mul_hz(factor);
        }
    }

    fn mod_hz(&mut self, factor: f64) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().mod_hz(factor);
        }
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().modify_amplitude(f.clone());
        }
    }
}

pub struct FourierWave(PolyWave);

impl FourierWave {
    pub fn new(coefficients: Vec<f32>, hz: f64) -> Self {
        let mut wwaves: Vec<ArcWave> = Vec::new();
        for (n, c) in coefficients.iter().enumerate() {
            let wp = WaveParams {
                hz: hz * n as f64,
                amplitude: *c,
                phase: 0.,
                hz0: hz * n as f64,
            };
            let s = SineWave(wp);
            wwaves.push(Arc::new(Mutex::new(s)));
        }
        FourierWave(PolyWave::new(wwaves, 1.))
    }

    pub fn boxed(coefficients: Vec<f32>, hz: f64) -> ArcMutex<Self> {
        arc(FourierWave::new(coefficients, hz))
    }
}

impl Wave for FourierWave {
    fn sample(&self) -> f32 {
        self.0.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.0.update_phase(sample_rate);
    }

    fn mul_hz(&mut self, factor: f64) {
        self.0.mul_hz(factor);
    }

    fn mod_hz(&mut self, factor: f64) {
        self.0.mod_hz(factor);
    }

    fn modify_amplitude(&mut self, f: Rc<dyn Fn(f32) -> f32>) {
        self.0.volume = f(self.0.volume);
    }
}

pub fn square_wave(n: u32, hz: f64) -> ArcMutex<FourierWave> {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            coefficients.push(1. / i as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierWave::boxed(coefficients, hz)
}

pub fn triangle_wave(n: u32, hz: f64) -> ArcMutex<FourierWave> {
    let mut coefficients: Vec<f32> = Vec::new();
    for i in 0..=n {
        if i % 2 == 1 {
            let sgn = if i % 4 == 1 { -1.0 } else { 1.0 };
            coefficients.push(sgn / (i * i) as f32);
        } else {
            coefficients.push(0.);
        }
    }
    FourierWave::boxed(coefficients, hz)
}
