use super::collections::*;
use super::containers::*;
use super::dsp::*;
use super::filters::*;

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
            (0.0, 2.0 * (knob - 0.5))
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
            self.lerp1.mtx().alpha = 2.0 * self.knob;
            self.lerp2.mtx().alpha = 0.0;
        } else {
            self.lerp1.mtx().alpha = 0.0;
            self.lerp2.mtx().alpha = 2.0 * (self.knob - 0.5);
        }
    }
}

impl Signal for WaveShaper {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        if self.knob <= 0.5 {
            self.lerp1.mtx().signal(sample_rate)
        } else {
            self.lerp2.mtx().signal(sample_rate)
        }
    }
}

impl HasHz for WaveShaper {
    fn hz(&self) -> Hz {
        0.0
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.lerp1.mtx().modify_hz(f);
        self.lerp2.mtx().modify_hz(f);
    }
}

pub struct ShaperOsc {
    pub fmsynth: FMSynth<WaveShaper, SineOsc>,
    pub ratio: Hz,
}

impl ShaperOsc {
    pub fn new(carrier_hz: Hz, ratio: Hz, mod_idx: Phase) -> Self {
        let shaper_osc = WaveShaper::wrapped(carrier_hz, 0.10);
        let sine_osc = SineOsc::wrapped(carrier_hz / ratio);
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
    fn signal(&mut self, sample_rate: f64) -> Amp {
        self.fmsynth.signal(sample_rate)
    }
}

pub struct Filter {
    pub lphp: BiquadFilter<TriggerSynth<ShaperOsc>>,
    pub cutoff: Hz,
    pub q: f64,
    pub t: f64,
}

impl Filter {
    pub fn new(lphp: BiquadFilter<TriggerSynth<ShaperOsc>>, cutoff: Hz, q: f64, t: f64) -> Self {
        Self { lphp, cutoff, q, t }
    }
}

pub struct ShaperSynth(pub Filter);

impl ShaperSynth {
    pub fn new(
        carrier_hz: Hz,
        ratio: Hz,
        mod_idx: Phase,
        attack: f32,
        decay: f32,
        sustain_time: f32,
        sustain_level: f32,
        release: f32,
        cutoff: Hz,
        q: f64,
        t: f64,
    ) -> Self {
        let wave = ShaperOsc::wrapped(carrier_hz, ratio, mod_idx);
        let triggeredwave =
            TriggerSynth::new(wave, attack, decay, sustain_time, sustain_level, release);
        let biquad = BiquadFilter::lphpf(arc(triggeredwave), 44_100., cutoff, q, t);
        let filter = Filter::new(biquad, cutoff, q, t);
        ShaperSynth(filter)
    }

    pub fn set_knob(&mut self, knob: f32) {
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().knob = knob;
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().set_alphas();
    }

    pub fn set_ratio(&mut self, ratio: Hz) {
        let base_hz = self.0.lphp.wave.mtx().wave.mtx().fmsynth.base_hz;
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.modulator.mtx().hz = base_hz / ratio;
    }

    pub fn set_carrier_hz(&mut self, hz: Hz) {
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().lerp1.mtx().wave1.mtx().hz = hz;
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().lerp1.mtx().wave2.mtx().hz = hz;
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().lerp2.mtx().wave1.mtx().hz = hz;
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.carrier.mtx().lerp2.mtx().wave2.mtx().hz = hz;
    }

    pub fn set_mod_idx(&mut self, mod_idx: Phase) {
        self.0.lphp.wave.mtx().wave.mtx().fmsynth.mod_idx = mod_idx;
    }

    pub fn set_cutoff(&mut self, cutoff: Hz) {
        self.0.cutoff = cutoff;
        let (b1, b2, a0, a1, a2) = lphpf(44_100., cutoff, self.0.q, self.0.t);
        self.0.lphp.b1 = b1;
        self.0.lphp.b2 = b2;
        self.0.lphp.a0 = a0;
        self.0.lphp.a1 = a1;
        self.0.lphp.a2 = a2;
    }

    pub fn set_q(&mut self, q: f64) {
        self.0.q = q;
        let (b1, b2, a0, a1, a2) = lphpf(44_100., self.0.cutoff, q, self.0.t);
        self.0.lphp.b1 = b1;
        self.0.lphp.b2 = b2;
        self.0.lphp.a0 = a0;
        self.0.lphp.a1 = a1;
        self.0.lphp.a2 = a2;
    }

    pub fn set_t(&mut self, t: f64) {
        self.0.t = t;
        let (b1, b2, a0, a1, a2) = lphpf(44_100., self.0.cutoff, self.0.q, t);
        self.0.lphp.b1 = b1;
        self.0.lphp.b2 = b2;
        self.0.lphp.a0 = a0;
        self.0.lphp.a1 = a1;
        self.0.lphp.a2 = a2;
    }

    pub fn set_attack(&mut self, attack: f32) {
        self.0.lphp.wave.mtx().attack = attack;
    }

    pub fn set_decay(&mut self, decay: f32) {
        self.0.lphp.wave.mtx().decay = decay;
    }

    pub fn set_sustain_time(&mut self, sustain_time: f32) {
        self.0.lphp.wave.mtx().sustain_time = sustain_time;
    }

    pub fn set_sustain_level(&mut self, sustain_level: f32) {
        self.0.lphp.wave.mtx().sustain_level = sustain_level;
    }

    pub fn set_release(&mut self, release: f32) {
        self.0.lphp.wave.mtx().release = release;
    }
}

impl Signal for ShaperSynth {
    fn signal(&mut self, sample_rate: f64) -> Amp {
        self.0.lphp.signal(sample_rate)
    }
}
