use crate::rack::*;
use crate::uti::ExpInterp;
use crate::{build, props, tag};

#[derive(Copy, Clone, Debug)]
pub struct Adsr {
    tag: Tag,
    clock: Real,
    sustain_time: Real,
    level: Real,
    a_param: Real,
    d_param: Real,
    r_param: Real,
    a_interp: ExpInterp,
    d_interp: ExpInterp,
    r_interp: ExpInterp,
}

impl Adsr {
    pub fn new(tag: Tag, a_param: Real, d_param: Real, r_param: Real) -> Self {
        let a_interp = ExpInterp::new(0.0, 0.5, 1.0);
        let d_interp = ExpInterp::new(0.0, 0.5, 1.0);
        let r_interp = ExpInterp::new(0.0, 0.5, 1.0);
        Self {
            tag,
            clock: 0.0,
            sustain_time: 0.0,
            level: 0.0,
            a_param,
            d_param,
            r_param,
            a_interp,
            d_interp,
            r_interp,
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

    pub fn calc_level(
        &mut self,
        controls: &Controls,
        outputs: &Outputs,
    ) -> Real {
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
            match self.clock {
                // Attack
                t if t < a => self.a_interp.interp(t / a),
                // Decay
                t if t < a + d => self.d_interp.interp((t - a) / d),
                // Sustain
                t => {
                    self.sustain_time = t - a - d;
                    s
                }
            }
        } else {
            match self.clock {
                // Attack
                t if t < a => self.a_interp.interp(t / a),
                // Decay
                t if t < a + d => self.d_interp.interp((t - a) / d),
                // Release
                t if t < a + d + r + self.sustain_time => {
                    self.r_interp.interp(t - a - d - self.sustain_time / r)
                }
                // Off
                _ => 0.0,
            }
        }
    }
    pub fn on(&mut self, controls: &mut Controls) {
        self.set_triggered(controls, true);
        self.sustain_time = 0.0;
        self.clock = self.a_interp.interp_inv(self.level);
    }
    pub fn off(&self, controls: &mut Controls) {
        self.set_triggered(controls, false);
    }
}

impl Signal for Adsr {
    tag!();
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, sample_rate: Real) {
        self.a_interp.update(0.0, 1.0 - self.a_param, 1.0);
        let s = self.sustain(controls, outputs);
        self.d_interp.update(1.0, s + self.d_param * (1.0 - s), s);
        self.r_interp.update(s, self.r_param * s, 0.0);
        self.level = self.calc_level(controls, outputs);
        outputs[(self.tag, 0)] = self.level;
        self.clock += 1.0 / sample_rate;
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls) -> Tag {
        let tag = rack.num_modules();
        controls[(tag, 0)] = self.attack;
        controls[(tag, 1)] = self.decay;
        controls[(tag, 2)] = self.sustain;
        controls[(tag, 3)] = self.release;
        controls[(tag, 4)] = self.triggered;
        let adsr = Box::new(Adsr::new(tag, self.a_param, self.d_param, self.r_param));
        rack.push(adsr);
        tag
    }
}
