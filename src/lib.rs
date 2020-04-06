use derive_more::Constructor;
use math::round::floor;
use nannou::prelude::*;

/// Creates a public struct and makes all fields public.
macro_rules! pub_struct {
    (
        $(#[derive($macros:tt)])*
        struct $name:ident {
            $($field:ident: $t:ty,)*
        }
    ) => {
        $(#[derive($macros)])*
        pub struct $name {
            $(pub $field: $t),*
        }
    }
}

/// Creates a wave with prepopulated boilerplate code for calling the
/// `WaveParams` methods. This shouldn't be used for any waves that need to
/// customize anything more than the sample function.
#[macro_export]
macro_rules! basic_wave {
    ($wave:ident, $sample:expr) => {
        pub struct $wave($crate::WaveParams);

        impl $wave {
            pub fn new(hz: f64, volume: f32, phase: f64) -> Self {
                $wave($crate::WaveParams::new(hz, volume, phase))
            }
        }

        impl Wave for $wave {
            fn sample(&self) -> f32 {
                $sample(self)
            }

            fn update_phase(&mut self, sample_rate: f64) {
                self.0.update_phase(sample_rate)
            }

            fn set_hz(&mut self, hz: f64) {
                self.0.set_hz(hz)
            }

            fn hz(&self) -> f64 {
                self.0.hz()
            }
        }
    };
}

pub trait Wave {
    fn sample(&self) -> f32;
    fn update_phase(&mut self, sample_rate: f64);
    fn set_hz(&mut self, hz: f64);
    fn hz(&self) -> f64;
}

pub_struct!(
    #[derive(Constructor)]
    struct WaveParams {
        hz: f64,
        volume: f32,
        phase: f64,
    }
);

impl WaveParams {
    pub fn update_phase(&mut self, sample_rate: f64) {
        self.phase += self.hz / sample_rate;
        self.phase %= sample_rate;
    }

    pub fn set_hz(&mut self, hz: f64) {
        self.hz = hz;
    }

    pub fn hz(&self) -> f64 {
        self.hz
    }
}

basic_wave!(SineWave, |wave: &SineWave| {
    wave.0.volume * (TAU * wave.0.phase as f32).sin()
});

basic_wave!(SquareWave, |wave: &SquareWave| {
    let sine_wave = SineWave(WaveParams::new(wave.0.hz, wave.0.volume, wave.0.phase));
    let sine_amp = sine_wave.sample();
    if sine_amp > 0. {
        wave.0.volume
    } else {
        -wave.0.volume
    }
});

basic_wave!(RampWave, |wave: &RampWave| {
    wave.0.volume * (2. * (wave.0.phase - floor(0.5 + wave.0.phase, 0))) as f32
});

basic_wave!(SawWave, |wave: &SawWave| {
    let t = wave.0.phase - 0.5;
    wave.0.volume * (2. * (-t - floor(0.5 - t, 0))) as f32
});

basic_wave!(TriangleWave, |wave: &TriangleWave| {
    let t = wave.0.phase - 0.75;
    let saw_amp = (2. * (-t - floor(0.5 - t, 0))) as f32;
    2. * saw_amp.abs() - wave.0.volume
});

#[derive(Constructor)]
pub struct LerpWave {
    wave1: Box<dyn Wave + Send>,
    wave2: Box<dyn Wave + Send>,
    alpha: f32,
}

impl Wave for LerpWave {
    fn sample(&self) -> f32 {
        (1. - self.alpha) * self.wave1.sample() + self.alpha * self.wave2.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.update_phase(sample_rate);
        self.wave2.update_phase(sample_rate);
    }

    fn set_hz(&mut self, hz: f64) {
        self.wave1.set_hz(hz);
        self.wave2.set_hz(hz);
    }

    //TODO: fix this...
    fn hz(&self) -> f64 {
        0.
    }
}

pub_struct!(
    struct MultWave {
        base_wave: Box<dyn Wave + Send>,
        mod_wave: Box<dyn Wave + Send>,
    }
);

impl Wave for MultWave {
    fn sample(&self) -> f32 {
        self.base_wave.sample() * self.mod_wave.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.base_wave.update_phase(sample_rate);
        self.mod_wave.update_phase(sample_rate);
    }

    fn set_hz(&mut self, hz: f64) {
        self.base_wave.set_hz(hz);
    }

    //TODO: fix this...
    fn hz(&self) -> f64 {
        self.base_wave.hz()
    }
}

pub_struct!(
    struct FMod {
        carrier_wave: Box<dyn Wave + Send>,
        mod_wave: Box<dyn Wave + Send>,
    }
);

impl Wave for FMod {
    fn sample(&self) -> f32 {
        self.carrier_wave.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.carrier_wave.update_phase(sample_rate);
        self.mod_wave.update_phase(sample_rate);

        self.carrier_wave
            .set_hz((self.mod_wave.sample() * 440.) as f64);
    }

    fn set_hz(&mut self, hz: f64) {
        self.carrier_wave.set_hz(hz);
    }

    //TODO: fix this...
    fn hz(&self) -> f64 {
        self.carrier_wave.hz()
    }
}

fn adsr(
    attack: f32,
    decay: f32,
    sustain_time: f32,
    sustain_level: f32,
    release: f32,
) -> Box<dyn Fn(f32) -> f32> {
    let a = attack * TAU;
    let d = decay * TAU;
    let s = sustain_time * TAU;
    let r = release * TAU;
    Box::new(move |t: f32| {
        let t = t % TAU;
        match t {
            x if x < a => t / a,
            x if x < a + d => 1.0 + (t - a) * (sustain_level - 1.0) / d,
            x if x < a + d + s => sustain_level,
            x if x < a + d + s + r => sustain_level - (t - a - d - s) * sustain_level / r,
            _ => 0.0,
        }
    })
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

impl Wave for ADSRWave {
    fn sample(&self) -> f32 {
        let f = adsr(
            self.attack,
            self.decay,
            self.sustain_time,
            self.sustain_level,
            self.release,
        );
        self.wave_params.volume * f(TAU * self.wave_params.phase as f32)
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave_params.update_phase(sample_rate);
    }

    fn set_hz(&mut self, hz: f64) {
        self.wave_params.set_hz(hz);
    }

    fn hz(&self) -> f64 {
        self.wave_params.hz()
    }
}

pub_struct!(
    struct AvgWave {
        waves: Vec<Box<dyn Wave + Send>>,
    }
);

impl Wave for AvgWave {
    fn sample(&self) -> f32 {
        self.waves.iter().fold(0.0, |acc, x| acc + x.sample()) / self.waves.len() as f32
    }

    fn update_phase(&mut self, sample_rate: f64) {
        for wave in self.waves.iter_mut() {
            wave.update_phase(sample_rate);
        }
    }

    fn set_hz(&mut self, hz: f64) {
        for wave in self.waves.iter_mut() {
            wave.set_hz(hz);
        }
    }

    fn hz(&self) -> f64 {
        self.waves.iter().fold(0.0, |acc, x| acc + x.hz()) / self.waves.len() as f64
    }
}
