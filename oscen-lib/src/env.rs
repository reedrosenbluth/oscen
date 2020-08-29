use crate::rack::*;
use crate::uti::{interp, interp_inv};
use crate::{build, props, tag};
use std::sync::Arc;

#[derive(Copy, Clone, Debug)]
pub struct Adsr {
    tag: Tag,
    a_param: Real,
    d_param: Real,
    r_param: Real,
}

impl Adsr {
    pub fn new(tag: Tag, a_param: Real, d_param: Real, r_param: Real) -> Self {
        Self {
            tag,
            a_param,
            d_param,
            r_param,
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
        state[(self.tag, 0)] = interp_inv(0.0, 1.0 - self.a_param, 1.0, x);
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
        sample_rate: Real,
    ) {
        fn max01(a: Real) -> Real {
            if a > 0.01 {
                a
            } else {
                0.01
            }
        }
        let a = max01(self.attack(controls, outputs));
        let d = max01(self.decay(controls, outputs));
        let s = self.sustain(controls, outputs);
        let r = max01(self.release(controls, outputs));
        let triggered = self.triggered(controls);
        if triggered {
            state[(self.tag, 2)] = match state[(self.tag, 0)] {
                // Attack
                t if t < a => interp(0.0, 1.0 - self.a_param, 1.0, t / a),
                // Decay
                t if t < a + d => interp(1.0, s + self.d_param * (1.0 - s), s, (t - a) / d),
                // Sustain
                t => {
                    state[(self.tag, 1)] = t - a - d;
                    s
                }
            }
        } else {
            state[(self.tag, 2)] = match state[(self.tag, 0)] {
                // Attack
                t if t < a => interp(0.0, 1.0 - self.a_param, 1.0, t / a),
                // Decay
                t if t < a + d => interp(1.0, s + self.d_param * (1.0 - s), s, (t - a) / d),
                // Release
                t if t < a + d + r + state[(self.tag, 1)] => interp(
                    s,
                    self.r_param * s,
                    0.0,
                    t - a - d - state[(self.tag, 1)] / r,
                ),
                // Off
                _ => 0.0,
            }
        }
        outputs[(self.tag, 0)] = state[(self.tag, 2)];
        state[(self.tag, 0)] += 1.0 / sample_rate;
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AdsrBuilder {
    a_param: Real,
    d_param: Real,
    r_param: Real,
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
            a_param: 0.5,
            d_param: 0.5,
            r_param: 0.5,
            attack,
            decay,
            sustain,
            release,
            triggered,
        }
    }
    pub fn linear() -> Self {
        let mut ab = Self::new();
        ab.a_param = 0.5;
        ab.d_param = 0.5;
        ab.r_param = 0.5;
        ab
    }
    pub fn exp_20() -> Self {
        let mut ab = Self::new();
        ab.a_param = 0.2;
        ab.d_param = 0.2;
        ab.r_param = 0.2;
        ab
    }
    build!(attack);
    build!(decay);
    build!(sustain);
    build!(release);

    pub fn a_param(&mut self, value: Real) -> &mut Self {
        self.a_param = value;
        self
    }
    pub fn d_param(&mut self, value: Real) -> &mut Self {
        self.d_param = value;
        self
    }
    pub fn r_param(&mut self, value: Real) -> &mut Self {
        self.r_param = value;
        self
    }
    pub fn triggered(&mut self, t: bool) -> &mut Self {
        self.triggered = t.into();
        self
    }
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Arc<Adsr> {
        let tag = Tag(rack.num_modules());
        controls[(tag, 0)] = self.attack;
        controls[(tag, 1)] = self.decay;
        controls[(tag, 2)] = self.sustain;
        controls[(tag, 3)] = self.release;
        controls[(tag, 4)] = self.triggered;
        let adsr = Arc::new(Adsr::new(tag, self.a_param, self.d_param, self.r_param));
        rack.push(adsr.clone());
        adsr
    }
}
