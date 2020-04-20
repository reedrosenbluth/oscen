use core::cmp::Ordering;
use core::time::Duration;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use nannou::prelude::*;
use nannou::ui::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;

use swell::collections::*;
use swell::containers::*;
use swell::dsp::*;

use widget::toggle::TimesClicked;
use widget::Toggle;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    stream: audio::Stream<Synth>,
    receiver: Receiver<f32>,
    amps: Vec<f32>,
    max_amp: f32,
    ui: Ui,
    ids: Vec<widget::Id>,
    wave_indices: Vec<f32>,
    squarewave: ArcMutex<FourierOsc>,
}

struct Synth {
    voice: Synth4<SineOsc, FourierOsc, BiquadFilter<SawOsc>, FMSynth<SineOsc, SineOsc>>,
    sender: Sender<f32>,
}

fn model(app: &App) -> Model {
    const HZ: f64 = 440.;
    let (sender, receiver) = unbounded();

    // Create a window to receive key pressed events.
    app.set_loop_mode(LoopMode::Rate {
        update_interval: Duration::from_millis(1),
    });
    app.new_window()
        .size(600, 340)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();
    let audio_host = audio::Host::new();
    let sine = SineOsc::wrapped(HZ);
    let square = square_wave(32, HZ);
    square.lock().unwrap().amplitude = 0.0;
    let saw = BiquadFilter::new(
        SawOsc::wrapped(HZ),
        -1.97615773,
        0.97643855,
        7.02037705e-5,
        1.40407541e-4,
        7.02037705e-5,
    );
    saw.wave.lock().unwrap().amplitude = 0.0;
    let triangle = TriangleOsc::wrapped(HZ);
    triangle.lock().unwrap().amplitude = 0.0;
    let carrier = SineOsc::wrapped(HZ);
    carrier.lock().unwrap().amplitude = 0.0;
    let modulator = SineOsc::wrapped(220.);
    let fm = FMSynth::wrapped(carrier, modulator, 3.0);

    let waves = Synth4::new(sine, square.clone(), arc(saw), fm);
    let num_waves = 4;
    let model = Synth {
        voice: waves,
        sender,
    };
    let stream = audio_host
        .new_output_stream(model)
        .render(audio)
        .build()
        .unwrap();

    let mut ui = app.new_ui().build().unwrap();
    let mut ids: Vec<widget::Id> = Vec::new();
    for _ in 0..num_waves {
        ids.push(ui.generate_widget_id());
    }

    let mut wave_indices = vec![0.; num_waves];
    wave_indices[0] = 1.0;

    Model {
        stream,
        receiver,
        amps: vec![],
        max_amp: 0.,
        ui,
        ids,
        wave_indices,
        squarewave: square.clone(),
    }
}

fn audio(synth: &mut Synth, buffer: &mut Buffer) {
    let sample_rate = buffer.sample_rate() as f64;
    for frame in buffer.frames_mut() {
        let mut amp = 0.;
        amp += synth.voice.signal(sample_rate);
        for channel in frame {
            *channel = amp;
        }
        synth.sender.send(amp).unwrap();
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    model.max_amp = 0.;
    let square_hz = model.squarewave.lock().unwrap().hz;
    let change_hz = |i| {
        model
            .stream
            .send(move |synth| {
                let factor = 2.0.powf(i / 12.);
                synth.voice.wave1.lock().unwrap().hz *= factor;
                synth.voice.wave2.lock().unwrap().set_hz(factor * square_hz);
                synth.voice.wave3.lock().unwrap().wave.lock().unwrap().hz *= factor;
                synth.voice.wave4.lock().unwrap().carrier.lock().unwrap().hz *= factor;
            })
            .unwrap();
    };
    match key {
        // Pause or unpause the audio when Space is pressed.
        Key::Space => {
            if model.stream.is_playing() {
                model.stream.pause().unwrap();
            } else {
                model.stream.play().unwrap();
            }
        }
        // Raise the frequency when the up key is pressed.
        Key::Up => change_hz(1.),
        Key::Down => change_hz(-1.),
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    let amps: Vec<f32> = model.receiver.try_iter().collect();
    let clone = amps.clone();

    // find max amplitude in waveform
    let max = amps.iter().max_by(|x, y| {
        if x > y {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    });

    // store if it's greater than the previously stored max
    if max.is_some() && *max.unwrap() > model.max_amp {
        model.max_amp = *max.unwrap();
    }
    model.amps = clone;

    let ui = &mut model.ui.set_widgets();

    let labels = &["Sine", "Square", "LPF(Saw)", "FM"];

    fn toggle(onoff: bool, lbl: &'static str) -> Toggle<'static> {
        Toggle::new(onoff)
            .w_h(100.0, 25.0)
            .label(lbl)
            .label_font_size(15)
            .rgb(9. / 255., 9. / 255., 44. / 255.)
            .label_color(if onoff {
                ui::color::WHITE
            } else {
                ui::color::DARK_RED
            })
            .border(0.0)
    }
    let mut toggles: Vec<TimesClicked> = Vec::new();
    let flags: Vec<bool> = model.wave_indices.iter().map(|x| *x > 0.).collect();

    for (i, f) in flags.iter().enumerate() {
        toggles.push(
            toggle(*f, labels[i])
                .top_left_with_margins(75.0 + 25.0 * i as f64, 20.)
                // .down(10.)
                .set(model.ids[i], ui),
        );
    }

    for (i, e) in toggles.iter_mut().enumerate() {
        for c in e {
            if c {
                if i == 2 { model.wave_indices[i] = 16.0}
                model.wave_indices[i] = 1.
            } else {
                model.wave_indices[i] = 0.
            }
        }
    }
    let ws = model.wave_indices.clone();
    let a = ws.iter().sum::<f32>();
    let ws: Vec<f32> = ws
        .iter()
        .map(|x| if a == 0.0 { 0.0 } else { x / a })
        .collect();
    model
        .stream
        .send(move |synth| {
            synth.voice.wave1.lock().unwrap().amplitude = ws[0];
            synth.voice.wave2.lock().unwrap().amplitude = ws[1];
            synth
                .voice
                .wave3
                .lock()
                .unwrap()
                .wave
                .lock()
                .unwrap()
                .amplitude = ws[2];
            synth
                .voice
                .wave4
                .lock()
                .unwrap()
                .carrier
                .lock()
                .unwrap()
                .amplitude = ws[3];
        })
        .unwrap();
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let c = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(c);
    let mut shifted: Vec<f32> = vec![];
    let mut iter = model.amps.iter().peekable();

    let mut i = 0;
    while iter.len() > 0 {
        let amp = iter.next().unwrap_or(&0.);
        if amp.abs() < 0.01 && **iter.peek().unwrap_or(&amp) > *amp {
            shifted = model.amps[i..].to_vec();
            break;
        }
        i += 1;
    }

    let l = 600;
    let mut points: Vec<Point2> = vec![];
    for (i, amp) in shifted.iter().enumerate() {
        if i == l {
            break;
        }
        points.push(pt2(i as f32, amp * 120.));
    }

    // only draw if we got enough info back from the audio thread
    if points.len() == 600 {
        draw.path()
            .stroke()
            .weight(2.)
            .points(points)
            .color(CORNFLOWERBLUE)
            .x_y(-150., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
    model.ui.draw_to_frame(app, &frame).unwrap();
}
