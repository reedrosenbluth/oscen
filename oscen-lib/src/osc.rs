use crate::rack::*;
use crate::tag;
use math::round::floor;
use std::f32::consts;

const TAU: f32 = 2.0 * consts::PI;

pub struct OscBuilder {
    signal_fn: fn(Real, Real) -> Real,
    phase: In,
    hz: In,
    amp: In,
    arg: In,
}

/// A standard oscillator that has phase, hz, and amp. Pass in a signal function
/// to operate on the phase and an optional extra argument.
#[derive(Clone)]
pub struct Oscillator {
    tag: Tag,
    phase: In,
    signal_fn: fn(Real, Real) -> Real,
}

pub fn oscillator(signal_fn: SignalFn) -> OscBuilder {
    OscBuilder::new(signal_fn)
}

impl OscBuilder {
    pub fn new(signal_fn: fn(Real, Real) -> Real) -> Self {
        Self {
            signal_fn,
            phase: 0.into(),
            hz: 0.into(),
            amp: 1.into(),
            arg: 0.5.into(),
        }
    }
    pub fn phase(&mut self, value: In) -> &mut Self {
        self.phase = value;
        self
    }
    pub fn hz(&mut self, value: In) -> &mut Self {
        self.hz = value;
        self
    }
    pub fn amp(&mut self, value: In) -> &mut Self {
        self.amp = value;
        self
    }
    pub fn arg(&mut self, value: In) -> &mut Self {
        self.arg = value;
        self
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack, controls: &mut Controls) -> Box<Oscillator> {
        let tag = rack.num_modules();
        controls.controls_mut(tag)[0] = self.hz;
        controls.controls_mut(tag)[1] = self.amp;
        controls.controls_mut(tag)[2] = self.arg;
        let osc = Box::new(Oscillator::new(tag, self.signal_fn));
        rack.0.push(osc.clone());
        osc
    }
}

pub fn sine_osc(phase: Real, _: Real) -> Real {
    (phase * TAU).sin()
}

pub fn square_osc(phase: Real, duty_cycle: Real) -> Real {
    let t = phase - phase.floor();
    if t <= duty_cycle {
        1.0
    } else {
        -1.0
    }
}

pub fn saw_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.5;
    let s = -t - floor(0.5 - t as f64, 0) as f32;
    if s < -0.5 {
        0.0
    } else {
        2.0 * s
    }
}

pub fn triangle_osc(phase: Real, _: Real) -> Real {
    let t = phase - 0.75;
    let saw_amp = 2. * (-t - floor(0.5 - t as f64, 0) as f32);
    2. * saw_amp.abs() - 1.0
}

impl Oscillator {
    pub fn new(tag: Tag, signal_fn: fn(Real, Real) -> Real) -> Self {
        Self {
            tag,
            phase: 0.into(),
            signal_fn,
        }
    }
    pub fn phase(&self, outputs: &Outputs) -> Real {
        outputs.value(self.phase)
    }
    pub fn set_phase(&mut self, value: In) {
        self.phase = value;
    }
    pub fn hz(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls.controls(self.tag)[0];
        outputs.value(inp)
    }
    pub fn set_hz(&self, controls: &mut Controls, value: In) {
        controls.controls_mut(self.tag)[0] = value;
    }
    pub fn amplitude(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls.controls(self.tag)[1];
        outputs.value(inp)
    }
    pub fn set_amplitude(&self, controls: &mut Controls, value: In) {
        controls.controls_mut(self.tag)[1] = value;
    }
    pub fn arg(&self, controls: &Controls, outputs: &Outputs) -> Real {
        let inp = controls.controls(self.tag)[2];
        outputs.value(inp)
    }
    pub fn set_arg(&self, controls: &mut Controls, value: In) {
        controls.controls_mut(self.tag)[2] = value;
    }
}

impl Signal for Oscillator {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, sample_rate: Real) {
        let phase = outputs.value(self.phase);
        let hz = self.hz(controls, outputs);
        let amp = self.amplitude(controls, outputs);
        let arg = self.arg(controls, outputs);
        match self.phase {
            In::Fix(p) => {
                let mut ph = p + hz / sample_rate;
                while ph >= 1.0 {
                    ph -= 1.0
                }
                while ph <= -1.0 {
                    ph += 1.0
                }
                self.phase = In::Fix(ph);
            }
            In::Cv(_, _) => {}
        };
        let outs = outputs.outputs_mut(self.tag);
        outs[0] = amp * (self.signal_fn)(phase, arg);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ConstBuilder {
    value: Real,
}

/// An synth module that returns a constant In value. Useful for example to
/// multiply or add constants to oscillators.
#[derive(Debug, Copy, Clone)]
pub struct Const {
    tag: Tag,
    value: Real,
}

impl ConstBuilder {
    pub fn new(value: Real) -> Self {
        Self { value }
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack, _controls: &mut Controls) -> Box<Const> {
        let tag = rack.num_modules();
        let out = Box::new(Const::new(tag, self.value));
        rack.0.push(out.clone());
        out
    }
}

impl Const {
    pub fn new(tag: Tag, value: Real) -> Self {
        Self { tag, value }
    }
}

impl Signal for Const {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let tag = self.tag();
        outputs.outputs_mut(tag)[0] = self.value;
    }
}

#[derive(Debug, Clone)]
pub struct Mixer {
    tag: Tag,
    waves: Vec<Tag>,
}

impl Mixer {
    pub fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
    pub fn rack<'a>(
        rack: &'a mut Rack,
        waves: Vec<Tag>,
    ) -> Box<Self> {
        let tag = rack.num_modules();
        let mix = Box::new(Self::new(tag, waves));
        rack.0.push(mix.clone());
        mix
    }
}

impl Signal for Mixer {
    tag!();
    fn signal(&mut self, _controls: &Controls, outputs: &mut Outputs, _sample_rate: Real) {
        let out = self
            .waves
            .iter()
            .fold(0.0, |acc, n| acc + outputs.outputs(*n)[0]);
        outputs.outputs_mut(self.tag)[0] = out;
    }
}
