use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use oscen::ops::*;
use oscen::osc::*;
use oscen::rack::*;

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

    let mut rack = Rack::new();
    let mut controls = Controls::new();
    let mut state = State::new();
    let mut outputs = Outputs::new();
    let mut oscs = vec![];
    let osc = OscBuilder::new(square_osc)
        .hz(440)
        .rack(&mut rack, &mut controls, &mut state);
    oscs.push(osc.tag());
    let mut builder = triangle_wave(32);
    builder.hz(220).lanczos(false);
    let osc = builder.rack(&mut rack, &mut controls);
    oscs.push(osc.tag());

    let _union = UnionBuilder::new(oscs).rack(&mut rack, &mut controls);

    // Produce a sinusoid of maximum amplitude.
    let mut next_value = move || rack.mono(&controls, &mut state, &mut outputs, sample_rate);

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
