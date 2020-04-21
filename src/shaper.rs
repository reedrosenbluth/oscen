use super::collections::*;
use super::containers::*;
use super::filters::*;
use super::dsp::*;

pub type ShaperSynth = BiquadFilter<TriggerSynth<ShaperOsc>>;
/// Interpolate between the three oscillators depending on the value of `knob`.
/// If `knob` is less thanb 1/2 then interpolate between square wave and sin wave,
/// otherwise interpolate between sine wave and saw wave.
pub struct WaveShaper {
    pub lerp1: ArcMutex<LerpSynth<SquareOsc, SineOsc>>,
    pub lerp2: ArcMutex<LerpSynth<SineOsc, SawOsc>>,
    pub knob: f32,
}

impl WaveShaper {
    pub fn new(hz: Hz, knob: f32) -> Self {
        let square = SquareOsc::wrapped(hz);
        let sine = SineOsc::wrapped(hz);
        let sine2 = sine.clone();
        let saw = SawOsc::wrapped(hz);
        let (a, b) = if knob <= 0.5 {
            (2.0 * knob, 0.0)
        } else {
            (0.0, 2.0 * (1.0 - knob))
        };
        let lerp1 = LerpSynth::wrapped(square, sine, a);
        let lerp2 = LerpSynth::wrapped(sine2, saw, b);
        WaveShaper { lerp1, lerp2, knob }
    }

    pub fn wrapped(hz: Hz, knob: f32) -> ArcMutex<Self> {
        arc(Self::new(hz, knob))
    }

    pub fn set_alphas(&mut self) {
        if self.knob <= 0.5 {
            self.lerp1.lock().unwrap().alpha = 2.0 * self.knob;
            self.lerp2.lock().unwrap().alpha = 0.0;
        } else {
            self.lerp1.lock().unwrap().alpha = 0.0;
            self.lerp2.lock().unwrap().alpha = 2.0 * (1.0 - self.knob);
        }
    }

    pub fn set_knob(&mut self, knob: f32) {
        self.knob = knob;
        self.set_alphas();
    }
}

impl Signal for WaveShaper {
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        if self.knob <= 0.5 {
            self.lerp1.lock().unwrap().signal_(sample_rate, add)
        } else {
            self.lerp2.lock().unwrap().signal_(sample_rate, add)
        }
    }
}

pub struct ShaperOsc {
    pub fmsynth: FMSynth<WaveShaper, SineOsc>,
    pub ratio: Hz,
}

impl ShaperOsc {
    pub fn new(carrier_hz: Hz, ratio: Hz, mod_idx: Phase) -> Self {
        let shaper_osc = WaveShaper::wrapped(carrier_hz, 0.10);
        let sine_osc = SineOsc::wrapped(ratio * carrier_hz);
        ShaperOsc {
            fmsynth: FMSynth::new(shaper_osc, sine_osc, mod_idx),
            ratio,
        }
    }

    pub fn wrapped(carrier_hz: Hz, ratio: Hz, mod_idx: Phase) -> ArcMutex<Self> {
        arc(Self::new(carrier_hz, ratio, mod_idx))
    }
}

impl Signal for ShaperOsc {
    fn signal_(&mut self, sample_rate: f64, add: Phase) -> Amp {
        self.fmsynth.signal_(sample_rate, add)
    }
}

pub fn shaper_osc(
    carrier_hz: Hz,
    ratio: Hz,
    mod_idx: Phase,
    attack: f32,
    decay: f32,
    sustain_time: f32,
    sustain_level: f32,
    release: f32,
    cutoff: Hz,
    q: f32,
) -> ShaperSynth {
    let wave = ShaperOsc::wrapped(carrier_hz, ratio, mod_idx);
    let triggeredwave = TriggerSynth::new(wave, attack, decay, sustain_time, sustain_level, release);
    BiquadFilter::lpf(arc(triggeredwave), 44100., cutoff, q)
}
