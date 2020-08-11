use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use math::round::floor;
use std::f32::consts::PI;
use std::sync::Arc;

type Real = f32;
type Tag = usize;

const TAU: f32 = 2.0 * PI;
#[derive(Debug, Clone)]
pub struct ModuleData {
    inputs: Vec<In>,
    parameters: Vec<Parameter>,
    buffer: Vec<Real>,
    outputs: Vec<Real>,
}

impl ModuleData {
    pub fn new(inputs: Vec<In>, outputs: Vec<Real>) -> Self {
        Self {
            inputs,
            parameters: vec![],
            buffer: vec![],
            outputs,
        }
    }
    pub fn parameters(&mut self, values: Vec<Parameter>) -> &mut Self {
        self.parameters = values;
        self
    }
    pub fn buffer(&mut self, values: Vec<Real>) -> &mut Self {
        self.buffer = values;
        self
    }
    pub fn build(&mut self) -> Self
    where
        Self: Clone,
    {
        self.to_owned()
    }
}

#[derive(Debug)]
pub struct ModuleTable {
    table: Vec<ModuleData>,
}

impl ModuleTable {
    pub fn new(table: Vec<ModuleData>) -> Self {
        Self { table }
    }

    pub fn inputs(&self, n: Tag) -> &[In] {
        self.table[n].inputs.as_slice()
    }
    pub fn inputs_mut(&mut self, n: Tag) -> &mut [In] {
        self.table[n].inputs.as_mut_slice()
    }
    pub fn outputs(&self, n: Tag) -> &[Real] {
        self.table[n].outputs.as_slice()
    }
    pub fn outputs_mut(&mut self, n: Tag) -> &mut [Real] {
        self.table[n].outputs.as_mut_slice()
    }
    pub fn value(&self, inp: In) -> Real {
        match inp {
            In::Fix(p) => p,
            In::Cv(n, i) => self.table[n].outputs[i],
        }
    }
}

pub trait Signal {
    fn tag(&self) -> Tag;
    fn modify_tag(&mut self, f: fn(Tag) -> Tag);
    fn signal(&self, modules: &mut ModuleTable, sample_rate: Real) -> Real;
}

#[derive(Copy, Clone, Debug)]
pub enum In {
    Cv(Tag, usize),
    Fix(Real),
}

#[derive(Copy, Clone, Debug)]
pub enum Parameter {
    Bool(bool),
    Int(i32),
    UInt(usize),
    Float(f64),
}

pub struct Rack(pub Vec<Arc<dyn Signal + Send + Sync>>);

impl Rack {
    pub fn play(&self, table: &mut ModuleTable, sample_rate: Real) -> Vec<Real> {
        let n = self.0.len() - 1;
        for module in self.0.iter() {
            module.signal(table, sample_rate);
        }
        table.outputs(n).to_owned()
    }
}

pub struct OscBuilder {
    signal_fn: fn(Real, Real) -> Real,
    phase: In,
    hz: In,
    amp: In,
    arg: In,
}

impl OscBuilder {
    pub fn new(signal_fn: fn(Real, Real) -> Real) -> Self {
        Self {
            signal_fn,
            phase: In::Fix(0.0),
            hz: In::Fix(0.0),
            amp: In::Fix(1.0),
            arg: In::Fix(0.5),
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
        table.table.push(data);
        let osc = Arc::new(Oscillator::new(tag, self.signal_fn));
        rack.0.push(osc.clone());
        osc
    }
}

pub fn sine_osc(phase: Real, _: Real) -> Real {
    (phase * TAU).sin()
}

pub fn square_osc(phase: Real, duty_cycle: Real) -> Real {
    let t = phase - floor(phase as f64, 0) as f32;
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
pub struct Oscillator {
    tag: Tag,
    signal_fn: fn(Real, Real) -> Real,
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
    fn tag(&self) -> Tag {
        self.tag
    }
    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.tag = f(self.tag);
    }
    fn signal(&self, table: &mut ModuleTable, sample_rate: Real) -> Real {
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
        outs[0]
    }
}

pub struct Mixer {
    tag: Tag,
    waves: Vec<Tag>,
}

impl Mixer {
    pub fn new(tag: Tag, waves: Vec<Tag>) -> Self {
        Self { tag, waves }
    }
    pub fn rack<'a>(rack: &'a mut Rack, table: &mut ModuleTable, waves: Vec<Tag>) -> Arc<Self> {
        let tag = rack.0.len();
        let inputs = vec![];
        let outputs = vec![0.0];
        let data = ModuleData::new(inputs, outputs);
        table.table.push(data);
        let mix = Arc::new(Self::new(tag, waves));
        rack.0.push(mix.clone());
        mix
    }
}

impl Signal for Mixer {
    fn tag(&self) -> Tag {
        self.tag
    }
    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.tag = f(self.tag);
    }
    fn signal(&self, modules: &mut ModuleTable, _sample_rate: Real) -> Real {
        let out = self
            .waves
            .iter()
            .fold(0.0, |acc, n| acc + modules.outputs(*n)[0]);
        modules.outputs_mut(self.tag)[0] = out;
        out
    }
}

fn main() -> Result<(), anyhow::Error> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    let config = device.default_output_config()?;

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, &config.into())?,
        cpal::SampleFormat::I16 => run::<i16>(&device, &config.into())?,
        cpal::SampleFormat::U16 => run::<u16>(&device, &config.into())?,
    }

    Ok(())
}

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), anyhow::Error>
where
    T: cpal::Sample,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    let mut rack = Rack(vec![]);
    let mut table = ModuleTable::new(vec![]);
    let num_oscillators = 900;
    let amp = 1.0 / num_oscillators as f32;
    let mut oscs = vec![];
    for _ in 0..num_oscillators {
        let sine = OscBuilder::new(saw_osc)
            .hz(In::Fix(440.0))
            .amp(In::Fix(amp))
            .rack(&mut rack, &mut table);
        oscs.push(sine.tag());
    }

    let _mixer = Mixer::rack(&mut rack, &mut table, oscs);
    dbg!(table.table.len());

    // Produce a sinusoid of maximum amplitude.
    let mut next_value = move || rack.play(&mut table, sample_rate)[0];

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(1000));

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: cpal::Sample,
{
    for frame in output.chunks_mut(channels) {
        let value: T = cpal::Sample::from::<f32>(&next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}
