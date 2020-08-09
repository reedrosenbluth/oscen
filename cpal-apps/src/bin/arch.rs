use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::f32::consts::PI;

type Real = f32;
type Tag = usize;

const TAU: f32 = 2.0 * PI;
#[derive(Debug)]
pub struct ModuleData {
    inputs: Vec<In>,
    outputs: Vec<Real>,
}

impl ModuleData {
    pub fn new(inputs: Vec<In>, outputs: Vec<Real>) -> Self {
        Self { inputs, outputs }
    }
}

#[derive(Debug)]
pub struct ModuleTable(Vec<ModuleData>);

impl ModuleTable {
    pub fn inputs(&self, n: Tag) -> &[In] {
        self.0[n].inputs.as_slice()
    }
    pub fn inputs_mut(&mut self, n: Tag) -> &mut [In] {
        self.0[n].inputs.as_mut_slice()
    }
    pub fn outputs(&self, n: Tag) -> &[Real] {
        self.0[n].outputs.as_slice()
    }
    pub fn outputs_mut(&mut self, n: Tag) -> &mut [Real] {
        self.0[n].outputs.as_mut_slice()
    }
    pub fn value(&self, inp: In) -> Real {
        match inp {
            In::Fix(p) => p,
            In::Cv(n, i) => self.0[n].outputs[i],
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

pub struct SineOsc {
    tag: Tag,
}

impl SineOsc {
    pub fn new(tag: Tag) -> Self {
        Self { tag }
    }
}

impl Signal for SineOsc {
    fn tag(&self) -> Tag {
        self.tag
    }
    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.tag = f(self.tag);
    }
    fn signal(&self, modules: &mut ModuleTable, sample_rate: Real) -> Real {
        let ins = modules.inputs(self.tag);
        let phase = modules.value(ins[0]);
        let hz = modules.value(ins[1]);
        let amp = modules.value(ins[2]);
        let ins = modules.inputs_mut(self.tag);
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
        let outs = modules.outputs_mut(self.tag);
        outs[0] = amp * (phase * TAU).sin();
        outs[0]
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

    let data = ModuleData {
        inputs: vec![In::Fix(0.0), In::Fix(440.0), In::Fix(1.0)],
        outputs: vec![0.0],
    };

    let mut modules = ModuleTable(vec![data]);
    let sine_osc = SineOsc::new(0);

    // Produce a sinusoid of maximum amplitude.
    let mut next_value = move || sine_osc.signal(&mut modules, sample_rate);

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
