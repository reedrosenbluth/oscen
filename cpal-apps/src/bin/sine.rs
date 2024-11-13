use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SizedSample, StreamConfig};
use iced::{
    mouse::Cursor,
    widget::{
        canvas::{Cache, Geometry, Path, Program},
        Canvas, Column,
    },
    Alignment, Application, Element, Length, Rectangle, Renderer, Settings, Theme,
};
use oscen::oscillators::{sine_osc, OscBuilder};
use oscen::rack::*;
use std::sync::mpsc::*;
use std::thread;

fn main() -> iced::Result {
    let (tx, rx) = channel();
    let (scope_tx, scope_rx) = channel();
    thread::spawn(|| {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("failed to find a default output device");
        let config = device.default_output_config()?;

        match config.sample_format() {
            cpal::SampleFormat::F32 => run::<f32>(&device, &config.into(), rx, scope_tx)?,
            cpal::SampleFormat::I16 => run::<i16>(&device, &config.into(), rx, scope_tx)?,
            cpal::SampleFormat::U16 => run::<u16>(&device, &config.into(), rx, scope_tx)?,
            _ => panic!("Unsupported sample format "),
        }
        Ok::<(), anyhow::Error>(())
    });

    let mut settings = Settings::with_flags((tx, scope_rx));
    settings.window.size = (405, 200);
    Model::run(settings)
}

pub fn run<T>(
    device: &Device,
    config: &StreamConfig,
    rx: Receiver<i32>,
    scope_tx: Sender<f32>,
) -> Result<(), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    let mut rack = Rack::default();

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

    let err_fn = |err| eprintln!("an error occurred on stream: {err}");

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value, &scope_tx)
        },
        err_fn,
        None,
    )?;
    stream.play()?;
    std::thread::sleep(std::time::Duration::from_millis(100000));
    Ok(())
}

fn write_data<T>(
    output: &mut [T],
    channels: usize,
    next_sample: &mut dyn FnMut() -> f32,
    scope_tx: &Sender<f32>,
) where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let v = next_sample();
        let _ = scope_tx.send(v);
        let value: T = T::from_sample(v);
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

struct Model {
    value: i32,
    tx: Sender<i32>,
    scope_rx: Receiver<f32>,
    cache: Cache,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    IncrementPressed,
    DecrementPressed,
}

impl iced::application::Application for Model {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = (Sender<i32>, Receiver<f32>);

    fn new(flags: (Sender<i32>, Receiver<f32>)) -> (Model, Command<Message>) {
        (
            Self {
                value: 0,
                tx: flags.0,
                scope_rx: flags.1,
                cache: Cache::default(),
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
        let canvas = Canvas::new(self).width(Length::Fill).height(Length::Fill);

        let buttons: Column<Message, Renderer> = column![
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
        row![buttons, Rule::vertical(10), freq, canvas].into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

impl Program<Message> for Model {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let geom = self.cache.draw(bounds.size(), |frame| {
            let circle = Path::circle(frame.center(), 50.0);
            frame.fill(&circle, iced::Color::from_rgb(0.8, 0.8, 0.1));
        });
        vec![geom]
    }
}

pub fn scope_data(data: &[f32]) -> Vec<f32> {
    let mut scope_data = data.iter().peekable();
    let mut shifted_scope_data: Vec<f32> = vec![];

    for (i, amp) in scope_data.clone().enumerate() {
        if *amp <= 0.0 && scope_data.peek().unwrap_or(&amp) > &&0.0 {
            shifted_scope_data = data[i..].to_vec();
            break;
        }
    }
    shifted_scope_data
}
