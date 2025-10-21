use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use oscen::filters::tpt::{TptFilter, TptFilterEndpoints};
use oscen::oscillators::PolyBlepOscillatorEndpoints;
use oscen::{Graph, PolyBlepOscillator, StreamOutput, ValueParam};
use slint::ComponentHandle;

slint::include_modules!();

/// Identifies each node in the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NodeId {
    SineOsc,
    SawOsc,
    Filter,
    Volume,
}

/// Identifies connection endpoints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointId {
    SineOut,
    SawOut,
    FilterIn,
    FilterOut,
    VolumeIn,
    VolumeOut,
}

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
}

#[derive(Default)]
struct VolumeConnection {
    active_source: Option<EndpointId>,
    active_output: Option<StreamOutput>,
    sine_route: Option<StreamOutput>,
    saw_route: Option<StreamOutput>,
    filter_route: Option<StreamOutput>,
}

impl VolumeConnection {
    fn activate(
        &mut self,
        source: EndpointId,
        graph: &mut Graph,
        endpoints: &NodeEndpoints,
    ) -> bool {
        let slot = match source {
            EndpointId::SineOut => &mut self.sine_route,
            EndpointId::SawOut => &mut self.saw_route,
            EndpointId::FilterOut => &mut self.filter_route,
            _ => return false,
        };

        let output = if let Some(route) = *slot {
            route
        } else {
            let route = match source {
                EndpointId::SineOut => {
                    graph.multiply(endpoints.sine_osc.output, endpoints.volume_param)
                }
                EndpointId::SawOut => {
                    graph.multiply(endpoints.saw_osc.output, endpoints.volume_param)
                }
                EndpointId::FilterOut => {
                    graph.multiply(endpoints.filter.output, endpoints.volume_param)
                }
                _ => unreachable!(),
            };
            *slot = Some(route);
            route
        };

        self.active_source = Some(source);
        self.active_output = Some(output);
        true
    }

    fn deactivate(&mut self, source: EndpointId) {
        if self.active_source == Some(source) {
            self.active_source = None;
            self.active_output = None;
        }
    }

    fn current_output(&self) -> Option<StreamOutput> {
        self.active_output
    }
}

/// Audio context containing the graph and all node endpoints
struct AudioContext {
    graph: Graph,
    endpoints: NodeEndpoints,
    volume_connection: VolumeConnection,
    connections: Vec<ConnectionState>,
    channels: usize,
}

impl AudioContext {
    fn new(sample_rate: f32, channels: usize) -> Self {
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

        // Create a value parameter for volume control
        let volume_param = graph.value_param(0.8);

        let endpoints = NodeEndpoints {
            sine_osc,
            saw_osc,
            filter,
            volume_param,
        };

        Self {
            graph,
            endpoints,
            volume_connection: VolumeConnection::default(),
            connections: Vec::new(),
            channels,
        }
    }

    fn apply_message(&mut self, msg: AudioMessage) {
        match msg {
            AudioMessage::AddConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if !self.connections.contains(&conn) && self.make_connection(from, to) {
                    self.connections.push(conn);
                }
            }
            AudioMessage::RemoveConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if self.remove_connection(from, to) {
                    self.connections.retain(|c| c != &conn);
                }
            }
            AudioMessage::SetSineFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.sine_osc.frequency, freq, 441);
            }
            AudioMessage::SetSawFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.saw_osc.frequency, freq, 441);
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
            (SineOut, VolumeIn) => {
                self.volume_connection
                    .activate(SineOut, &mut self.graph, &self.endpoints)
            }
            (SawOut, FilterIn) => {
                self.graph
                    .connect(self.endpoints.saw_osc.output, self.endpoints.filter.input);
                true
            }
            (SawOut, VolumeIn) => {
                self.volume_connection
                    .activate(SawOut, &mut self.graph, &self.endpoints)
            }
            (FilterOut, VolumeIn) => {
                self.volume_connection
                    .activate(FilterOut, &mut self.graph, &self.endpoints)
            }
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
            (SawOut, FilterIn) => self
                .graph
                .disconnect(self.endpoints.saw_osc.output, self.endpoints.filter.input),
            (SineOut, VolumeIn) => {
                self.volume_connection.deactivate(SineOut);
                true
            }
            (SawOut, VolumeIn) => {
                self.volume_connection.deactivate(SawOut);
                true
            }
            (FilterOut, VolumeIn) => {
                self.volume_connection.deactivate(FilterOut);
                true
            }
            _ => {
                eprintln!("Invalid disconnection: {:?} -> {:?}", from, to);
                false
            }
        }
    }

    fn get_output(&mut self) -> Result<f32> {
        self.graph.process()?;

        // Read from the volume output if something is connected, otherwise return 0
        let value = if let Some(output) = self.volume_connection.current_output() {
            self.graph.get_value(&output).unwrap_or(0.0)
        } else {
            0.0
        };

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
        let mut audio_context = AudioContext::new(sample_rate, config.channels as usize);

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

    run_ui(msg_tx, connections)?;
    Ok(())
}

fn run_ui(tx: Sender<AudioMessage>, connections: Arc<Mutex<Vec<ConnectionState>>>) -> Result<()> {
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
        5 => Some(EndpointId::VolumeOut),
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
        EndpointId::VolumeOut => 5,
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

        // Set active connections
        for conn in conns.iter() {
            match (conn.from, conn.to) {
                (EndpointId::SineOut, EndpointId::FilterIn) => ui.set_conn_sine_filter(true),
                (EndpointId::SineOut, EndpointId::VolumeIn) => ui.set_conn_sine_volume(true),
                (EndpointId::SawOut, EndpointId::FilterIn) => ui.set_conn_saw_filter(true),
                (EndpointId::SawOut, EndpointId::VolumeIn) => ui.set_conn_saw_volume(true),
                (EndpointId::FilterOut, EndpointId::VolumeIn) => ui.set_conn_filter_volume(true),
                _ => {}
            }
        }
    }
}
