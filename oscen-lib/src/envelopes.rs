use crate::rack::*;
use crate::utils::{arc_mutex, interp, interp_inv, ArcMutex};
use crate::{build, props, tag};

#[derive(Copy, Clone, Debug)]
pub struct Adsr {
    tag: Tag,
    ax: f32,
    dx: f32,
    rx: f32,
    x: f32,
    y: f32,
    z: f32,
}

impl Adsr {
    pub fn new<T: Into<Tag>>(tag: T, ax: f32, dx: f32, rx: f32) -> Self {
        Self {
            tag: tag.into(),
            ax,
            dx,
            rx,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    props!(attack, set_attack, 0);
    props!(decay, set_decay, 1);
    props!(sustain, set_sustain, 2);
    props!(release, set_release, 3);

    pub fn triggered(&self, rack: &Rack) -> bool {
        let ctrl = rack.controls[(self.tag, 4)];
        match ctrl {
            Control::B(b) => b,
            _ => panic!("triggered must be a bool, not {ctrl:?}"),
        }
    }

    pub fn set_triggered(&self, rack: &mut Rack, value: bool) {
        rack.controls[(self.tag, 4)] = value.into();
    }

    pub fn on(&mut self, rack: &mut Rack) {
        self.set_triggered(rack, true);
        self.y = 0.0;
        self.x = interp_inv(0.0, 1.0 - self.ax, 1.0, self.z);
    }

    pub fn off(&self, rack: &mut Rack) {
        self.set_triggered(rack, false);
    }
}

impl Signal for Adsr {
    tag!();
    fn signal(&mut self, rack: &mut Rack, sample_rate: f32) {
        let a = self.attack(rack).max(0.005);
        let d = self.decay(rack).max(0.005);
        let s = self.sustain(rack);
        let r = self.release(rack).max(0.005);
        let triggered = self.triggered(rack);
        self.z = match (triggered, self.x) {
            (_, t) if t < a => interp(0.0, 1.0 - self.ax, 1.0, t / a),
            (_, t) if t < a + d => interp(1.0, s + self.dx * (1.0 - s), s, (t - a) / d),
            (true, t) => {
                self.y = t - a - d;
                s
            }
            (false, t) if t < a + d + r + self.y => {
                interp(s, self.rx * s, 0.0, t - a - d - self.y / r)
            }
            (false, _) => 0.0,
        };
        self.x = self.z;
        self.x += 1.0 / sample_rate;
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

impl Default for AdsrBuilder {
    fn default() -> Self {
        let attack = 0.01.into();
        let decay = 0.0.into();
        let sustain = 1.0.into();
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
}

impl AdsrBuilder {
    pub fn new() -> Self {
        Self::default()
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
    pub fn rack(&self, rack: &mut Rack) -> ArcMutex<Adsr> {
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.attack;
        rack.controls[(n, 1)] = self.decay;
        rack.controls[(n, 2)] = self.sustain;
        rack.controls[(n, 3)] = self.release;
        rack.controls[(n, 4)] = self.triggered;
        let adsr = arc_mutex(Adsr::new(n, self.ax, self.dx, self.rx));
        rack.push(adsr.clone());
        adsr
    }
}
