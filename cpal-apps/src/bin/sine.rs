use anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SizedSample, StreamConfig};
use iced::widget::row;
use iced::{
    widget::{button, column, text, Rule},
    Alignment, Application, Command, Element, Settings, Theme,
};
use oscen::oscillators::{sine_osc, OscBuilder};
use oscen::rack::*;
use std::sync::mpsc::*;
use std::thread;

fn main() -> iced::Result {
    let (tx, rx) = channel();
    thread::spawn(|| {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("failed to find a default output device");
        let config = device.default_output_config()?;

        match config.sample_format() {
            cpal::SampleFormat::F32 => run::<f32>(&device, &config.into(), rx)?,
            cpal::SampleFormat::I16 => run::<i16>(&device, &config.into(), rx)?,
            cpal::SampleFormat::U16 => run::<u16>(&device, &config.into(), rx)?,
            _ => panic!("Unsupported sample format "),
        }
        Ok::<(), anyhow::Error>(())
    });

    let mut settings = Settings::with_flags(tx);
    settings.window.size = (405, 200);
    Model::run(settings)
}

pub fn run<T>(
    device: &Device,
    config: &StreamConfig,
    rx: Receiver<i32>,
) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    let mut rack = Rack::default();
    // let mut storage = Storage::default();

    let so = OscBuilder::new(sine_osc)
        .hz(220.0)
        .amplitude(0.25)
        .rack(&mut rack);

    let mut next_value = move || {
        if let Ok(r) = rx.try_recv() {
            so.set_hz(&mut rack, (220.0 * 1.059463_f32.powf(r as f32)).into());
        };
        rack.mono(sample_rate)
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

struct Model {
    value: i32,
    tx: Sender<i32>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    IncrementPressed,
    DecrementPressed,
}

impl Application for Model {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = Sender<i32>;

    fn new(flags: Sender<i32>) -> (Model, Command<Message>) {
        (
            Self {
                value: 0,
                tx: flags,
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Sine Wave")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::IncrementPressed => {
                self.value += 1;
            }
            Message::DecrementPressed => {
                self.value -= 1;
            }
        }
        let _ = self.tx.send(self.value);
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let buttons = column![
            button(
                text("+")
                    .width(50)
                    .horizontal_alignment(iced::alignment::Horizontal::Center)
            )
            .on_press(Message::IncrementPressed),
            text(self.value).size(50),
            button(
                text("-")
                    .width(50)
                    .horizontal_alignment(iced::alignment::Horizontal::Center)
            )
            .on_press(Message::DecrementPressed)
        ]
        .padding(30)
        .spacing(10)
        .align_items(Alignment::Center);
        let freq = column![text(format!(
            "Frequency: {:.0}",
            220.0 * 1.059463_f32.powf(self.value as f32)
        ))
        .size(35)]
        .padding(30);
        row![buttons, Rule::vertical(10), freq].into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}
