use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use iced::widget::shader::wgpu::hal::auxil::db;
use oscen::operators::*;
use oscen::oscillators::*;
use oscen::rack::*;
use std::{env, sync::Arc};

fn synth(rack: &mut Rack) -> Arc<Union> {
    let mut tags = vec![];

    // Sine 0
    let freq = 330.0;
    let sine = OscBuilder::new(sine_osc).hz(freq).rack(rack);
    tags.push(sine.lock().unwrap().tag());

    // Square 1
    let square = OscBuilder::new(square_osc).hz(freq).rack(rack);
    tags.push(square.lock().unwrap().tag());

    // Triangle 2
    let tri = OscBuilder::new(triangle_osc).hz(freq).rack(rack);
    tags.push(tri.lock().unwrap().tag());

    // Fourier Square 8. 3
    let mut builder = square_wave(8);
    builder.hz(freq);
    let sq8 = builder.rack(rack);
    tags.push(sq8.lock().unwrap().tag());

    // Fourier tri 8. 4
    let mut builder = triangle_wave(8);
    builder.hz(freq);
    let tri8 = builder.rack(rack);
    tags.push(tri8.lock().unwrap().tag());

    // PinkNoise 5
    let pn = PinkNoiseBuilder::new().amplitude(0.5).rack(rack);
    tags.push(pn.lock().unwrap().tag());

    // FM 6
    let modulator = ModulatorBuilder::new(sine_osc)
        .hz(220.0)
        .ratio(2.0)
        .index(4.0)
        .rack(rack);
    let fm = OscBuilder::new(triangle_osc)
        .hz(modulator.lock().unwrap().tag())
        .rack(rack);
    tags.push(fm.lock().unwrap().tag());

    // LFO
    let lfo = OscBuilder::new(sine_osc).hz(2.0).rack(rack);

    // Vca, where amplitude is controlled by lfo. 7
    let vca = VcaBuilder::new(sine.lock().unwrap().tag())
        .level(lfo.lock().unwrap().tag())
        .rack(rack);
    tags.push(vca.lock().unwrap().tag());

    // CrossFade 8
    let cf =
        CrossFadeBuilder::new(sine.lock().unwrap().tag(), square.lock().unwrap().tag()).rack(rack);
    cf.lock()
        .unwrap()
        .set_alpha(rack, Control::V(lfo.lock().unwrap().tag(), 0));
    tags.push(cf.lock().unwrap().tag());

    // Delay 9
    let delay = DelayBuilder::new(sine.lock().unwrap().tag(), 0.02.into()).rack(rack);
    let d =
        CrossFadeBuilder::new(sine.lock().unwrap().tag(), delay.lock().unwrap().tag()).rack(rack);
    tags.push(d.lock().unwrap().tag());

    Arc::new(UnionBuilder::new(tags).rack(rack).lock().unwrap().clone())
}

fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = env::args().collect();
    let tag_num = (&args[1]).parse::<usize>()?;
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("failed to find a default output device");
    let config = device.default_output_config()?;

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, &config.into(), tag_num)?,
        cpal::SampleFormat::I16 => run::<i16>(&device, &config.into(), tag_num)?,
        cpal::SampleFormat::U16 => run::<u16>(&device, &config.into(), tag_num)?,
        _ => panic!("Unsupported sample format "),
    }

    Ok(())
}

fn run<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tag_num: usize,
) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    let mut rack = Rack::default();

    let union = synth(&mut rack);
    union.set_active(&mut rack, tag_num.into());

    let mut next_value = move || rack.mono(sample_rate);

    let err_fn = |err| eprintln!("an error occurred on stream: {err}");

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    std::thread::sleep(std::time::Duration::from_millis(100000));

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}
