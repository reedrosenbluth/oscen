use crate::rack::*;
use crate::utils::{interp, interp_inv};
use crate::{build, props, tag};
use std::sync::Arc;

#[derive(Copy, Clone, Debug)]
pub struct Adsr {
    tag: Tag,
    ax: f32,
    dx: f32,
    rx: f32,
}

impl Adsr {
    pub fn new<T: Into<Tag>>(tag: T, ax: f32, dx: f32, rx: f32) -> Self {
        Self {
            tag: tag.into(),
            ax,
            dx,
            rx,
        }
    }
    props!(attack, set_attack, 0);
    props!(decay, set_decay, 1);
    props!(sustain, set_sustain, 2);
    props!(release, set_release, 3);

    pub fn triggered(&self, controls: &Controls) -> bool {
        let ctrl = controls[(self.tag, 4)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("triggered must be a bool, not {:?}", ctrl),
        }
    }
    pub fn set_triggered(&self, controls: &mut Controls, value: bool) {
        controls[(self.tag, 4)] = value.into();
    }

    pub fn on(&self, controls: &mut Controls, state: &mut State) {
        self.set_triggered(controls, true);
        state[(self.tag, 1)] = 0.0;
        let x = state[(self.tag, 2)];
        state[(self.tag, 0)] = interp_inv(0.0, 1.0 - self.ax, 1.0, x);
    }
    pub fn off(&self, controls: &mut Controls) {
        self.set_triggered(controls, false);
    }
}

impl Signal for Adsr {
    tag!();
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let a = self.attack(controls, outputs).max(0.005);
        let d = self.decay(controls, outputs).max(0.005);
        let s = self.sustain(controls, outputs);
        let r = self.release(controls, outputs).max(0.005);
        let triggered = self.triggered(controls);
        state[(self.tag, 2)] = match (triggered, state[(self.tag, 0)]) {
            (_, t) if t < a => interp(0.0, 1.0 - self.ax, 1.0, t / a),
            (_, t) if t < a + d => interp(1.0, s + self.dx * (1.0 - s), s, (t - a) / d),
            (true, t) => {
                state[(self.tag, 1)] = t - a - d;
                s
            }
            (false, t) if t < a + d + r + state[(self.tag, 1)] => {
                interp(s, self.rx * s, 0.0, t - a - d - state[(self.tag, 1)] / r)
            }
            (false, _) => 0.0,
        };
        outputs[(self.tag, 0)] = state[(self.tag, 2)];
        state[(self.tag, 0)] += 1.0 / sample_rate;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AdsrBuilder {
    ax: f32,
    dx: f32,
    rx: f32,
    attack: Control,
    decay: Control,
    sustain: Control,
    release: Control,
    triggered: Control,
}

impl AdsrBuilder {
    pub fn new() -> Self {
        let attack = 0.01.into();
        let decay = 0.into();
        let sustain = 1.into();
        let release = 0.1.into();
        let triggered = false.into();
        Self {
            ax: 0.5,
            dx: 0.5,
            rx: 0.5,
            attack,
            decay,
            sustain,
            release,
            triggered,
        }
    }
    pub fn linear() -> Self {
        let mut ab = Self::new();
        ab.ax = 0.5;
        ab.dx = 0.5;
        ab.rx = 0.5;
        ab
    }
    pub fn exp_20() -> Self {
        let mut ab = Self::new();
        ab.ax = 0.2;
        ab.dx = 0.2;
        ab.rx = 0.2;
        ab
    }
    build!(attack);
    build!(decay);
    build!(sustain);
    build!(release);

    pub fn ax(&mut self, value: f32) -> &mut Self {
        self.ax = value;
        self
    }
    pub fn dx(&mut self, value: f32) -> &mut Self {
        self.dx = value;
        self
    }
    pub fn rx(&mut self, value: f32) -> &mut Self {
        self.rx = value;
        self
    }
    pub fn triggered(&mut self, t: bool) -> &mut Self {
        self.triggered = t.into();
        self
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Adsr> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.attack;
        controls[(n, 1)] = self.decay;
        controls[(n, 2)] = self.sustain;
        controls[(n, 3)] = self.release;
        controls[(n, 4)] = self.triggered;
        let adsr = Arc::new(Adsr::new(n, self.ax, self.dx, self.rx));
        rack.push(adsr.clone());
        adsr
    }
}
