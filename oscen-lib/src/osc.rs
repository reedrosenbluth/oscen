use crate::rack::*;
use crate::tag;
use math::round::floor;
use std::{f32::consts, sync::Arc};

const TAU: f32 = 2.0 * consts::PI;

pub struct OscBuilder {
    signal_fn: fn(Real, Real) -> Real,
    phase: In,
    hz: In,
    amp: In,
    arg: In,
}

pub struct Oscillator {
    tag: Tag,
    signal_fn: fn(Real, Real) -> Real,
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
    pub fn rack<'a>(&self, rack: &'a mut Rack, table: &mut ModuleTable) -> Arc<Oscillator> {
        let tag = rack.0.len();
        let inputs = vec![self.phase, self.hz, self.amp, self.arg];
        let outputs = vec![0.0];
        let data = ModuleData::new(inputs, outputs);
        table.push(data);
        let osc = Arc::new(Oscillator::new(tag, self.signal_fn));
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
        Self { tag, signal_fn }
    }
    pub fn phase(&self, table: &ModuleTable) -> Real {
        let inp = table.inputs(self.tag)[0];
        table.value(inp)
    }
    pub fn set_phase(&self, table: &mut ModuleTable, value: In) {
        table.inputs_mut(self.tag)[0] = value;
    }
    pub fn hz(&self, table: &ModuleTable) -> Real {
        let inp = table.inputs(self.tag)[1];
        table.value(inp)
    }
    pub fn set_hz(&self, table: &mut ModuleTable, value: In) {
        table.inputs_mut(self.tag)[1] = value;
    }
    pub fn amplitude(&self, table: &ModuleTable) -> Real {
        let inp = table.inputs(self.tag)[2];
        table.value(inp)
    }
    pub fn set_amplitude(&self, table: &mut ModuleTable, value: In) {
        table.inputs_mut(self.tag)[2] = value;
    }
    pub fn arg(&self, table: &ModuleTable) -> Real {
        let inp = table.inputs(self.tag)[3];
        table.value(inp)
    }
    pub fn set_arg(&self, table: &mut ModuleTable, value: In) {
        table.inputs_mut(self.tag)[3] = value;
    }
}

impl Signal for Oscillator {
    tag!();
    fn signal(&self, table: &mut ModuleTable, sample_rate: Real) {
        let phase = self.phase(table);
        let hz = self.hz(table);
        let amp = self.amplitude(table);
        let arg = self.arg(table);
        let ins = table.inputs_mut(self.tag);
        match ins[0] {
            In::Fix(p) => {
                let mut ph = p + hz / sample_rate;
                while ph >= 1.0 {
                    ph -= 1.0
                }
                while ph <= -1.0 {
                    ph += 1.0
                }
                ins[0] = In::Fix(ph);
            }
            In::Cv(_, _) => {}
        };
        let outs = table.outputs_mut(self.tag);
        outs[0] = amp * (self.signal_fn)(phase, arg);
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ConstBuilder {
    value: Real,
}

#[derive(Debug, Copy, Clone)]
pub struct Const {
    tag: Tag,
}

impl ConstBuilder {
    pub fn new(value: Real) -> Self {
        Self { value }
    }
    pub fn rack<'a>(&self, rack: &'a mut Rack, table: &mut ModuleTable) -> Arc<Const> {
        let tag = rack.0.len();
        let outputs = vec![self.value];
        let data = ModuleData::new(vec![], outputs);
        table.push(data);
        let out = Arc::new(Const::new(tag));
        rack.0.push(out.clone());
        out
    }
}

impl Const {
    pub fn new(tag: Tag) -> Self {
        Self { tag }
    }
}

impl Signal for Const {
    tag!();
    fn signal(&self, _modules: &mut ModuleTable, _sample_rate: Real) {
    }
}
