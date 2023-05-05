use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use iced::{
    widget::{button, column, text},
    Alignment, Element, Sandbox, Settings,
};
use oscen::oscillators::{sine_osc, OscBuilder};
use oscen::rack::*;
use std::thread;

// fn main() -> Result<(), anyhow::Error> {
fn main() -> iced::Result {
    thread::spawn(|| {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("failed to find a default output device");
        let config = device.default_output_config()?;

        match config.sample_format() {
            cpal::SampleFormat::F32 => run::<f32>(&device, &config.into())?,
            cpal::SampleFormat::I16 => run::<i16>(&device, &config.into())?,
            cpal::SampleFormat::U16 => run::<u16>(&device, &config.into())?,
            _ => panic!("Unsupported sample format "),
        }
        Ok::<(), anyhow::Error>(())
    });

    Counter::run(Settings::default())
}

pub fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();

    OscBuilder::new(sine_osc)
        .hz(330.0)
        .rack(&mut rack, &mut controls, &mut state);

    let mut next_value = move || {
        rack.mono(
            &controls,
            &mut state,
            &mut outputs,
            &mut buffers,
            sample_rate,
        )
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

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

struct Counter {
    value: i32,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    IncrementPressed,
    DecrementPressed,
}

impl Sandbox for Counter {
    type Message = Message;

    fn new() -> Self {
        Self { value: 0 }
    }

    fn title(&self) -> String {
        String::from("Counter - Iced")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::IncrementPressed => {
                self.value += 1;
            }
            Message::DecrementPressed => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> Element<Message> {
        column![
            button("Increment").on_press(Message::IncrementPressed),
            text(self.value).size(50),
            button("Decrement").on_press(Message::DecrementPressed)
        ]
        .padding(20)
        .align_items(Alignment::Center)
        .into()
    }
}
