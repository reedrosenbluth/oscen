use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::filters::tpt::{TptFilter, TptFilterEndpoints};
use oscen::oscillators::PolyBlepOscillatorEndpoints;
use oscen::{
    Gain, GainEndpoints, Graph, Oscilloscope, OscilloscopeEndpoints, OscilloscopeHandle,
    PolyBlepOscillator, StreamOutput, ValueParam, DEFAULT_SCOPE_CAPACITY,
};
use slint::{ComponentHandle, Image, Rgb8Pixel, SharedPixelBuffer, Timer, TimerMode};

slint::include_modules!();

/// Identifies each node in the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NodeId {
    SineOsc,
    SawOsc,
    Filter,
    Output,
    Oscilloscope,
}

/// Identifies connection endpoints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointId {
    SineOut,
    SawOut,
    FilterIn,
    FilterOut,
    VolumeIn,
    ScopeIn,
    ScopeOut,
}

const SCOPE_IMAGE_WIDTH: u32 = 160;
const SCOPE_IMAGE_HEIGHT: u32 = 120;
const SCOPE_BACKGROUND: [u8; 3] = [27, 36, 32];
const SCOPE_AXIS_COLOR: [u8; 3] = [60, 72, 68];
const SCOPE_WAVE_COLOR: [u8; 3] = [138, 198, 255];

/// Represents a connection between two endpoints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConnectionState {
    from: EndpointId,
    to: EndpointId,
}

/// Messages from UI thread to audio thread
#[derive(Debug, Clone)]
enum AudioMessage {
    AddConnection(EndpointId, EndpointId),
    RemoveConnection(EndpointId, EndpointId),
    SetSineFreq(f32),
    SetSawFreq(f32),
    SetFilterCutoff(f32),
    SetFilterQ(f32),
    SetVolumeLevel(f32),
}

/// Stores the endpoints for each node
struct NodeEndpoints {
    sine_osc: PolyBlepOscillatorEndpoints,
    saw_osc: PolyBlepOscillatorEndpoints,
    filter: TptFilterEndpoints,
    volume_param: ValueParam,
    oscilloscope: OscilloscopeEndpoints,
    gain: GainEndpoints,
}

/// Audio context containing the graph and all node endpoints
struct AudioContext {
    graph: Graph,
    endpoints: NodeEndpoints,
    connections: Vec<ConnectionState>,
    channels: usize,
    scope_handle: OscilloscopeHandle,
    scope_period_param: ValueParam,
    scope_enable_param: ValueParam,
    scope_source: Option<EndpointId>,
    sample_rate: f32,
    sine_freq: f32,
    saw_freq: f32,
}

impl AudioContext {
    fn new(sample_rate: f32, channels: usize, scope_handle: OscilloscopeHandle) -> Self {
        let mut graph = Graph::new(sample_rate);

        // Create nodes with fixed parameters
        let sine_osc = graph.add_node(PolyBlepOscillator::sine(
            220.0, // A3
            0.5,   // amplitude
        ));

        let saw_osc = graph.add_node(PolyBlepOscillator::saw(
            440.0, // A4
            0.5,   // amplitude
        ));

        let filter = graph.add_node(TptFilter::new(
            1000.0, // cutoff
            0.707,  // Q
        ));

        let oscilloscope_node = Oscilloscope::with_handle(scope_handle.clone());
        let oscilloscope = graph.add_node(oscilloscope_node);

        // Create gain node for volume control
        let volume_param = graph.value_param(0.8);
        let gain = graph.add_node(Gain::new(0.8));
        graph.connect(volume_param, gain.gain);

        let scope_period_param = graph.value_param(DEFAULT_SCOPE_CAPACITY as f32);
        let scope_enable_param = graph.value_param(1.0);

        graph.connect(scope_period_param, oscilloscope.trigger_period);
        graph.connect(scope_enable_param, oscilloscope.trigger_enabled);

        let endpoints = NodeEndpoints {
            sine_osc,
            saw_osc,
            filter,
            volume_param,
            oscilloscope,
            gain,
        };

        let mut context = Self {
            graph,
            endpoints,
            connections: Vec::new(),
            channels,
            scope_handle,
            scope_period_param,
            scope_enable_param,
            scope_source: None,
            sample_rate,
            sine_freq: 220.0,
            saw_freq: 440.0,
        };

        context.update_scope_period();
        context
    }

    fn apply_message(&mut self, msg: AudioMessage) {
        match msg {
            AudioMessage::AddConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if !self.connections.contains(&conn) && self.make_connection(from, to) {
                    if to == EndpointId::VolumeIn {
                        self.connections.retain(|c| c.to != EndpointId::VolumeIn);
                    }
                    if to == EndpointId::ScopeIn {
                        self.connections.retain(|c| c.to != EndpointId::ScopeIn);
                    }
                    self.connections.push(conn);
                    self.update_scope_period();
                }
            }
            AudioMessage::RemoveConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if self.remove_connection(from, to) {
                    self.connections.retain(|c| c != &conn);
                    self.update_scope_period();
                }
            }
            AudioMessage::SetSineFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.sine_osc.frequency, freq, 441);
                self.sine_freq = freq;
                self.update_scope_period();
            }
            AudioMessage::SetSawFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.saw_osc.frequency, freq, 441);
                self.saw_freq = freq;
                self.update_scope_period();
            }
            AudioMessage::SetFilterCutoff(cutoff) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.filter.cutoff, cutoff, 1323);
            }
            AudioMessage::SetFilterQ(q) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.filter.q, q, 1323);
            }
            AudioMessage::SetVolumeLevel(level) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.volume_param, level, 441);
            }
        }
    }

    fn make_connection(&mut self, from: EndpointId, to: EndpointId) -> bool {
        use EndpointId::*;

        match (from, to) {
            (SineOut, FilterIn) => {
                self.graph
                    .connect(self.endpoints.sine_osc.output, self.endpoints.filter.input);
                true
            }
            (SineOut, VolumeIn) => self.set_volume_source(SineOut),
            (SineOut, ScopeIn) => {
                self.graph.connect(
                    self.endpoints.sine_osc.output,
                    self.endpoints.oscilloscope.input,
                );
                self.scope_source = Some(SineOut);
                true
            }
            (SawOut, FilterIn) => {
                self.graph
                    .connect(self.endpoints.saw_osc.output, self.endpoints.filter.input);
                true
            }
            (SawOut, VolumeIn) => self.set_volume_source(SawOut),
            (SawOut, ScopeIn) => {
                self.graph.connect(
                    self.endpoints.saw_osc.output,
                    self.endpoints.oscilloscope.input,
                );
                self.scope_source = Some(SawOut);
                true
            }
            (FilterOut, ScopeIn) => {
                self.graph.connect(
                    self.endpoints.filter.output,
                    self.endpoints.oscilloscope.input,
                );
                self.scope_source = Some(FilterOut);
                true
            }
            (FilterOut, VolumeIn) => self.set_volume_source(FilterOut),
            (ScopeOut, VolumeIn) => self.set_volume_source(ScopeOut),
            _ => {
                eprintln!("Invalid connection: {:?} -> {:?}", from, to);
                false
            }
        }
    }

    fn remove_connection(&mut self, from: EndpointId, to: EndpointId) -> bool {
        use EndpointId::*;

        match (from, to) {
            (SineOut, FilterIn) => self
                .graph
                .disconnect(self.endpoints.sine_osc.output, self.endpoints.filter.input),
            (SineOut, ScopeIn) => {
                let result = self.graph.disconnect(
                    self.endpoints.sine_osc.output,
                    self.endpoints.oscilloscope.input,
                );
                if result && self.scope_source == Some(SineOut) {
                    self.scope_source = None;
                }
                result
            }
            (SawOut, FilterIn) => self
                .graph
                .disconnect(self.endpoints.saw_osc.output, self.endpoints.filter.input),
            (SawOut, ScopeIn) => {
                let result = self.graph.disconnect(
                    self.endpoints.saw_osc.output,
                    self.endpoints.oscilloscope.input,
                );
                if result && self.scope_source == Some(SawOut) {
                    self.scope_source = None;
                }
                result
            }
            (SineOut, VolumeIn) => self.clear_volume_source(SineOut),
            (SawOut, VolumeIn) => self.clear_volume_source(SawOut),
            (FilterOut, ScopeIn) => {
                let result = self.graph.disconnect(
                    self.endpoints.filter.output,
                    self.endpoints.oscilloscope.input,
                );
                if result && self.scope_source == Some(FilterOut) {
                    self.scope_source = None;
                }
                result
            }
            (FilterOut, VolumeIn) => self.clear_volume_source(FilterOut),
            (ScopeOut, VolumeIn) => self.clear_volume_source(ScopeOut),
            _ => {
                eprintln!("Invalid disconnection: {:?} -> {:?}", from, to);
                false
            }
        }
    }

    fn current_scope_frequency(&self) -> Option<f32> {
        match self.scope_source? {
            EndpointId::SineOut => Some(self.sine_freq),
            EndpointId::SawOut => Some(self.saw_freq),
            EndpointId::FilterOut => {
                for conn in self.connections.iter().rev() {
                    match (conn.from, conn.to) {
                        (EndpointId::SineOut, EndpointId::FilterIn) => return Some(self.sine_freq),
                        (EndpointId::SawOut, EndpointId::FilterIn) => return Some(self.saw_freq),
                        _ => {}
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn update_scope_period(&mut self) {
        if let Some(freq) = self.current_scope_frequency().filter(|f| *f > 0.0) {
            let max_len = self.scope_handle.capacity() as f32;
            let period_samples = (self.sample_rate / freq).clamp(1.0, max_len);
            self.graph
                .set_value(self.scope_period_param, period_samples);
            self.graph.set_value(self.scope_enable_param, 1.0);
        } else {
            self.graph.set_value(self.scope_enable_param, 0.0);
            self.scope_handle.clear_triggered();
        }
    }

    fn stream_for_endpoint(&self, endpoint: EndpointId) -> Option<StreamOutput> {
        match endpoint {
            EndpointId::SineOut => Some(self.endpoints.sine_osc.output),
            EndpointId::SawOut => Some(self.endpoints.saw_osc.output),
            EndpointId::FilterOut => Some(self.endpoints.filter.output),
            EndpointId::ScopeOut => Some(self.endpoints.oscilloscope.output),
            _ => None,
        }
    }

    fn current_volume_source(&self) -> Option<EndpointId> {
        self.connections
            .iter()
            .find(|c| c.to == EndpointId::VolumeIn)
            .map(|c| c.from)
    }

    fn set_volume_source(&mut self, source: EndpointId) -> bool {
        if let Some(prev) = self.current_volume_source() {
            if let Some(prev_stream) = self.stream_for_endpoint(prev) {
                self.graph
                    .disconnect(prev_stream, self.endpoints.gain.input);
            }
        }

        if let Some(stream) = self.stream_for_endpoint(source) {
            self.graph.connect(stream, self.endpoints.gain.input);
            true
        } else {
            false
        }
    }

    fn clear_volume_source(&mut self, source: EndpointId) -> bool {
        if self.current_volume_source() == Some(source) {
            if let Some(stream) = self.stream_for_endpoint(source) {
                self.graph.disconnect(stream, self.endpoints.gain.input);
            }
        }
        true
    }

    fn get_output(&mut self) -> Result<f32> {
        self.graph.process()?;

        let value = self
            .graph
            .get_value(&self.endpoints.gain.output)
            .unwrap_or(0.0);

        Ok(value)
    }
}

fn audio_callback(data: &mut [f32], context: &mut AudioContext, msg_rx: &Receiver<AudioMessage>) {
    // Process incoming messages from UI
    while let Ok(msg) = msg_rx.try_recv() {
        context.apply_message(msg);
    }

    // Generate audio samples
    for frame in data.chunks_mut(context.channels) {
        let value = match context.get_output() {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Graph processing error: {}", err);
                0.0
            }
        };

        // Only output to first 2 channels (stereo pair)
        for (i, sample) in frame.iter_mut().enumerate() {
            *sample = if i < 2 { value } else { 0.0 };
        }
    }
}

fn main() -> Result<()> {
    let (msg_tx, msg_rx) = mpsc::channel();

    // Store active connections for UI display
    let connections = Arc::new(Mutex::new(Vec::<ConnectionState>::new()));
    let scope_handle = OscilloscopeHandle::new(DEFAULT_SCOPE_CAPACITY);
    let audio_scope_handle = scope_handle.clone();

    thread::spawn(move || {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(device) => device,
            None => {
                eprintln!("No output device available");
                return;
            }
        };

        let default_config = match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Failed to fetch default output config: {}", err);
                return;
            }
        };

        let config = cpal::StreamConfig {
            channels: default_config.channels(),
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(512),
        };

        let sample_rate = config.sample_rate.0 as f32;
        let mut audio_context =
            AudioContext::new(sample_rate, config.channels as usize, audio_scope_handle);

        let stream = match device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                audio_callback(data, &mut audio_context, &msg_rx);
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        ) {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("Failed to build output stream: {}", err);
                return;
            }
        };

        if let Err(err) = stream.play() {
            eprintln!("Failed to start audio stream: {}", err);
            return;
        }

        loop {
            thread::sleep(Duration::from_millis(100));
        }
    });

    run_ui(msg_tx, connections, scope_handle)?;
    Ok(())
}

fn run_ui(
    tx: Sender<AudioMessage>,
    connections: Arc<Mutex<Vec<ConnectionState>>>,
    scope_handle: OscilloscopeHandle,
) -> Result<()> {
    let ui = ModularWindow::new()?;

    // Handle connection requests from UI
    {
        let tx = tx.clone();
        let connections = connections.clone();
        let ui_weak = ui.as_weak();
        ui.on_connect(move |from_id, to_id| {
            println!("Connect callback triggered: {} -> {}", from_id, to_id);
            let from = endpoint_from_id(from_id);
            let to = endpoint_from_id(to_id);

            if let (Some(from), Some(to)) = (from, to) {
                println!("Creating connection: {:?} -> {:?}", from, to);
                let _ = tx.send(AudioMessage::AddConnection(from, to));

                // Update UI connection list
                if let Ok(mut conns) = connections.lock() {
                    let conn = ConnectionState { from, to };
                    if !conns.contains(&conn) {
                        conns.push(conn);
                    }
                }

                // Update UI visual connections
                if let Some(ui) = ui_weak.upgrade() {
                    update_ui_connections(&ui, &connections);
                }
            }
        });
    }

    // Handle disconnection requests from UI
    {
        let tx = tx.clone();
        let connections = connections.clone();
        let ui_weak = ui.as_weak();
        ui.on_disconnect(move |from_id, to_id| {
            let from = endpoint_from_id(from_id);
            let to = endpoint_from_id(to_id);

            if let (Some(from), Some(to)) = (from, to) {
                let _ = tx.send(AudioMessage::RemoveConnection(from, to));

                // Update UI connection list
                if let Ok(mut conns) = connections.lock() {
                    let conn = ConnectionState { from, to };
                    conns.retain(|c| c != &conn);
                }

                // Update UI visual connections
                if let Some(ui) = ui_weak.upgrade() {
                    update_ui_connections(&ui, &connections);
                }
            }
        });
    }

    // Handle parameter changes
    {
        let tx = tx.clone();
        ui.on_sine_freq_changed(move |freq| {
            let _ = tx.send(AudioMessage::SetSineFreq(freq));
        });
    }

    {
        let tx = tx.clone();
        ui.on_saw_freq_changed(move |freq| {
            let _ = tx.send(AudioMessage::SetSawFreq(freq));
        });
    }

    {
        let tx = tx.clone();
        ui.on_filter_cutoff_changed(move |cutoff| {
            let _ = tx.send(AudioMessage::SetFilterCutoff(cutoff));
        });
    }

    {
        let tx = tx.clone();
        ui.on_filter_q_changed(move |q| {
            let _ = tx.send(AudioMessage::SetFilterQ(q));
        });
    }

    {
        let tx = tx.clone();
        ui.on_volume_level_changed(move |level| {
            let _ = tx.send(AudioMessage::SetVolumeLevel(level));
        });
    }

    let scope_handle_for_timer = scope_handle.clone();
    let scope_timer = Timer::default();
    let ui_weak_for_timer = ui.as_weak();
    scope_timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
        if let Some(ui) = ui_weak_for_timer.upgrade() {
            let buffer = render_scope_waveform(
                &scope_handle_for_timer,
                SCOPE_IMAGE_WIDTH,
                SCOPE_IMAGE_HEIGHT,
            );
            ui.set_scope_waveform(Image::from_rgb8(buffer));
        }
    });

    ui.run().context("failed to run UI")
}

/// Convert UI endpoint ID to internal EndpointId
fn endpoint_from_id(id: i32) -> Option<EndpointId> {
    match id {
        0 => Some(EndpointId::SineOut),
        1 => Some(EndpointId::SawOut),
        2 => Some(EndpointId::FilterIn),
        3 => Some(EndpointId::FilterOut),
        4 => Some(EndpointId::VolumeIn),
        5 => Some(EndpointId::ScopeIn),
        6 => Some(EndpointId::ScopeOut),
        _ => None,
    }
}

/// Convert internal EndpointId to UI endpoint ID
fn endpoint_to_id(endpoint: EndpointId) -> i32 {
    match endpoint {
        EndpointId::SineOut => 0,
        EndpointId::SawOut => 1,
        EndpointId::FilterIn => 2,
        EndpointId::FilterOut => 3,
        EndpointId::VolumeIn => 4,
        EndpointId::ScopeIn => 5,
        EndpointId::ScopeOut => 6,
    }
}

/// Update the UI's visual connection list
fn update_ui_connections(ui: &ModularWindow, connections: &Arc<Mutex<Vec<ConnectionState>>>) {
    if let Ok(conns) = connections.lock() {
        println!("Updating UI with {} connections", conns.len());

        // Reset all connections
        ui.set_conn_sine_filter(false);
        ui.set_conn_sine_volume(false);
        ui.set_conn_saw_filter(false);
        ui.set_conn_saw_volume(false);
        ui.set_conn_filter_volume(false);
        ui.set_conn_sine_scope(false);
        ui.set_conn_saw_scope(false);
        ui.set_conn_filter_scope(false);
        ui.set_conn_scope_output(false);

        // Set active connections
        for conn in conns.iter() {
            match (conn.from, conn.to) {
                (EndpointId::SineOut, EndpointId::FilterIn) => ui.set_conn_sine_filter(true),
                (EndpointId::SineOut, EndpointId::VolumeIn) => ui.set_conn_sine_volume(true),
                (EndpointId::SineOut, EndpointId::ScopeIn) => ui.set_conn_sine_scope(true),
                (EndpointId::SawOut, EndpointId::FilterIn) => ui.set_conn_saw_filter(true),
                (EndpointId::SawOut, EndpointId::VolumeIn) => ui.set_conn_saw_volume(true),
                (EndpointId::SawOut, EndpointId::ScopeIn) => ui.set_conn_saw_scope(true),
                (EndpointId::FilterOut, EndpointId::VolumeIn) => ui.set_conn_filter_volume(true),
                (EndpointId::FilterOut, EndpointId::ScopeIn) => ui.set_conn_filter_scope(true),
                (EndpointId::ScopeOut, EndpointId::VolumeIn) => ui.set_conn_scope_output(true),
                _ => {}
            }
        }
    }
}

fn render_scope_waveform(
    handle: &OscilloscopeHandle,
    width: u32,
    height: u32,
) -> SharedPixelBuffer<Rgb8Pixel> {
    let width = width.max(1);
    let height = height.max(1);
    let width_usize = width as usize;
    let height_usize = height as usize;

    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
    let snapshot = handle.snapshot((width_usize * 4).max(width_usize));

    {
        let pixels = buffer.make_mut_slice();
        fill_background(pixels, SCOPE_BACKGROUND);
        draw_axis(pixels, width_usize, height_usize, SCOPE_AXIS_COLOR);
        let samples = if !snapshot.triggered().is_empty() {
            snapshot.triggered()
        } else {
            snapshot.samples()
        };
        draw_waveform(pixels, width_usize, height_usize, samples, SCOPE_WAVE_COLOR);
    }

    buffer
}

fn fill_background(pixels: &mut [Rgb8Pixel], color: [u8; 3]) {
    for px in pixels.iter_mut() {
        *px = Rgb8Pixel::new(color[0], color[1], color[2]);
    }
}

fn draw_axis(pixels: &mut [Rgb8Pixel], width: usize, height: usize, color: [u8; 3]) {
    if height == 0 {
        return;
    }
    let y = height / 2;
    for x in 0..width {
        let idx = y * width + x;
        pixels[idx] = Rgb8Pixel::new(color[0], color[1], color[2]);
    }
}

fn draw_waveform(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    samples: &[f32],
    color: [u8; 3],
) {
    if width == 0 || height == 0 || samples.is_empty() {
        return;
    }

    let center = (height as f32 - 1.0) / 2.0;
    let scale = center * 0.85;
    let mut prev_y = sample_to_y(samples[0], center, scale, height);
    plot_pixel(pixels, width, height, 0, prev_y, color);

    for x in 1..width {
        let t = x as f32 / (width - 1) as f32;
        let sample = sample_at(samples, t);
        let current_y = sample_to_y(sample, center, scale, height);
        draw_line_segment(
            pixels,
            width,
            height,
            (x - 1) as i32,
            prev_y,
            x as i32,
            current_y,
            color,
        );
        prev_y = current_y;
    }
}

fn sample_at(samples: &[f32], t: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let max_index = (samples.len() - 1) as f32;
    let position = t * max_index;
    let idx0 = position.floor() as usize;
    let idx1 = position.ceil().min((samples.len() - 1) as f32) as usize;
    let frac = position - idx0 as f32;
    let s0 = samples[idx0];
    let s1 = samples[idx1];
    s0 + (s1 - s0) * frac
}

fn sample_to_y(sample: f32, center: f32, scale: f32, height: usize) -> i32 {
    let clamped = sample.clamp(-1.0, 1.0);
    let y = center - clamped * scale;
    y.clamp(0.0, height as f32 - 1.0).round() as i32
}

fn plot_pixel(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: [u8; 3],
) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = (y as usize) * width + (x as usize);
    pixels[idx] = Rgb8Pixel::new(color[0], color[1], color[2]);
}

fn draw_line_segment(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 3],
) {
    let mut x0 = x0.clamp(0, width as i32 - 1);
    let mut y0 = y0.clamp(0, height as i32 - 1);
    let x1 = x1.clamp(0, width as i32 - 1);
    let y1 = y1.clamp(0, height as i32 - 1);

    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    loop {
        plot_pixel(pixels, width, height, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
}
