mod scope_renderer;

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
    PolyBlepOscillator, StreamInput, StreamOutput, ValueParam, DEFAULT_SCOPE_CAPACITY,
};
use slint::{ComponentHandle, Image, Timer, TimerMode};

slint::include_modules!();

const SCOPE_IMAGE_WIDTH: u32 = 320;
const SCOPE_IMAGE_HEIGHT: u32 = 120;

/// Represents a connection between two endpoints (using integer IDs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConnectionState {
    from: i32,
    to: i32,
}

/// Information about a jack's position for cable drawing
#[derive(Debug, Clone)]
struct JackPositionInfo {
    id: i32,
    x: f32,
    y: f32,
    is_input: bool,
}

/// Messages from UI thread to audio thread
#[derive(Debug, Clone)]
enum UIMessage {
    AddConnection(i32, i32),
    RemoveConnection(i32, i32),
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
}

impl AudioContext {
    fn new(sample_rate: f32, channels: usize, scope_handle: OscilloscopeHandle) -> Self {
        let mut graph = Graph::new(sample_rate);

        // Create nodes with fixed parameters
        let sine_osc = graph.add_node(PolyBlepOscillator::sine(
            220.0, // A3
            0.35,  // amplitude
        ));

        let saw_osc = graph.add_node(PolyBlepOscillator::saw(
            440.0, // A4
            0.35,  // amplitude
        ));

        let filter = graph.add_node(TptFilter::new(
            1000.0, // cutoff
            0.707,  // Q
        ));

        let oscilloscope_node = Oscilloscope::with_auto_detect(scope_handle.clone());
        let oscilloscope = graph.add_node(oscilloscope_node);

        // Create gain node for volume control
        let volume_param = graph.value_param(0.8);
        let gain = graph.add_node(Gain::new(0.8));
        graph.connect(volume_param, gain.gain);

        let endpoints = NodeEndpoints {
            sine_osc,
            saw_osc,
            filter,
            volume_param,
            oscilloscope,
            gain,
        };

        Self {
            graph,
            endpoints,
            connections: Vec::new(),
            channels,
        }
    }

    fn apply_message(&mut self, msg: UIMessage) {
        match msg {
            UIMessage::AddConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if !self.connections.contains(&conn) && self.make_connection(from, to) {
                    self.connections.push(conn);
                }
            }
            UIMessage::RemoveConnection(from, to) => {
                let conn = ConnectionState { from, to };
                if self.remove_connection(from, to) {
                    self.connections.retain(|c| c != &conn);
                }
            }
            UIMessage::SetSineFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.sine_osc.frequency, freq, 441);
            }
            UIMessage::SetSawFreq(freq) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.saw_osc.frequency, freq, 441);
            }
            UIMessage::SetFilterCutoff(cutoff) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.filter.cutoff, cutoff, 1323);
            }
            UIMessage::SetFilterQ(q) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.filter.q, q, 1323);
            }
            UIMessage::SetVolumeLevel(level) => {
                self.graph
                    .set_value_with_ramp(self.endpoints.volume_param, level, 441);
            }
        }
    }

    fn get_stream_output(&self, id: i32) -> Option<StreamOutput> {
        match id {
            0 => Some(self.endpoints.sine_osc.output),     // SineOut
            1 => Some(self.endpoints.saw_osc.output),      // SawOut
            3 => Some(self.endpoints.filter.output),       // FilterOut
            6 => Some(self.endpoints.oscilloscope.output), // ScopeOut
            _ => None,
        }
    }

    fn get_stream_input(&self, id: i32) -> Option<StreamInput> {
        match id {
            2 => Some(self.endpoints.filter.input),       // FilterIn
            4 => Some(self.endpoints.gain.input),         // VolumeIn
            5 => Some(self.endpoints.oscilloscope.input), // ScopeIn
            _ => None,
        }
    }

    fn make_connection(&mut self, from: i32, to: i32) -> bool {
        if let (Some(output), Some(input)) =
            (self.get_stream_output(from), self.get_stream_input(to))
        {
            self.graph.connect(output, input);
            true
        } else {
            eprintln!("Invalid connection: {:?} -> {:?}", from, to);
            false
        }
    }

    fn remove_connection(&mut self, from: i32, to: i32) -> bool {
        if let (Some(output), Some(input)) =
            (self.get_stream_output(from), self.get_stream_input(to))
        {
            self.graph.disconnect(output, input)
        } else {
            eprintln!("Invalid disconnection: {:?} -> {:?}", from, to);
            false
        }
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

fn audio_callback(data: &mut [f32], context: &mut AudioContext, msg_rx: &Receiver<UIMessage>) {
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
    tx: Sender<UIMessage>,
    connections: Arc<Mutex<Vec<ConnectionState>>>,
    scope_handle: OscilloscopeHandle,
) -> Result<()> {
    let ui = ModularWindow::new()?;

    // Initialize jack positions based on initial node positions
    update_jack_positions(&ui);

    // Handle connection requests from UI
    {
        let tx = tx.clone();
        let connections = connections.clone();
        let ui_weak = ui.as_weak();
        ui.on_connect(move |from_id, to_id| {
            println!("Connect callback triggered: {} -> {}", from_id, to_id);
            println!("Creating connection: {} -> {}", from_id, to_id);
            let _ = tx.send(UIMessage::AddConnection(from_id, to_id));

            // Update UI connection list
            if let Ok(mut conns) = connections.lock() {
                let conn = ConnectionState {
                    from: from_id,
                    to: to_id,
                };
                if !conns.contains(&conn) {
                    conns.push(conn);
                }
            }

            // Update UI visual connections
            if let Some(ui) = ui_weak.upgrade() {
                update_ui_connections(&ui, &connections);
            }
        });
    }

    // Handle disconnection requests from UI
    {
        let tx = tx.clone();
        let connections = connections.clone();
        let ui_weak = ui.as_weak();
        ui.on_disconnect(move |from_id, to_id| {
            let _ = tx.send(UIMessage::RemoveConnection(from_id, to_id));

            // Update UI connection list
            if let Ok(mut conns) = connections.lock() {
                let conn = ConnectionState {
                    from: from_id,
                    to: to_id,
                };
                conns.retain(|c| c != &conn);
            }

            // Update UI visual connections
            if let Some(ui) = ui_weak.upgrade() {
                update_ui_connections(&ui, &connections);
            }
        });
    }

    // Handle parameter changes
    {
        let tx = tx.clone();
        ui.on_sine_freq_changed(move |freq| {
            let _ = tx.send(UIMessage::SetSineFreq(freq));
        });
    }

    {
        let tx = tx.clone();
        ui.on_saw_freq_changed(move |freq| {
            let _ = tx.send(UIMessage::SetSawFreq(freq));
        });
    }

    {
        let tx = tx.clone();
        ui.on_filter_cutoff_changed(move |cutoff| {
            let _ = tx.send(UIMessage::SetFilterCutoff(cutoff));
        });
    }

    {
        let tx = tx.clone();
        ui.on_filter_q_changed(move |q| {
            let _ = tx.send(UIMessage::SetFilterQ(q));
        });
    }

    {
        let tx = tx.clone();
        ui.on_volume_level_changed(move |level| {
            let _ = tx.send(UIMessage::SetVolumeLevel(level));
        });
    }

    // Handle disconnect_all_from_output callback
    {
        let tx = tx.clone();
        let connections = connections.clone();
        let ui_weak = ui.as_weak();
        ui.on_disconnect_all_from_output(move |from_id| {
            if let Ok(conns) = connections.lock() {
                // Find all connections from this output and disconnect them
                let to_disconnect: Vec<_> = conns
                    .iter()
                    .filter(|c| c.from == from_id)
                    .cloned()
                    .collect();

                drop(conns); // Release lock before sending messages

                for conn in to_disconnect {
                    let _ = tx.send(UIMessage::RemoveConnection(conn.from, conn.to));

                    // Update UI connection list
                    if let Ok(mut conns) = connections.lock() {
                        conns.retain(|c| c != &conn);
                    }
                }

                // Update UI visual connections
                if let Some(ui) = ui_weak.upgrade() {
                    update_ui_connections(&ui, &connections);
                }
            }
        });
    }

    // Handle node_moved callback - update jack positions when nodes are dragged
    {
        let ui_weak = ui.as_weak();
        ui.on_node_moved(move || {
            if let Some(ui) = ui_weak.upgrade() {
                update_jack_positions(&ui);
            }
        });
    }

    let scope_handle_for_timer = scope_handle.clone();
    let scope_timer = Timer::default();
    let ui_weak_for_timer = ui.as_weak();
    scope_timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
        if let Some(ui) = ui_weak_for_timer.upgrade() {
            let buffer = scope_renderer::render_waveform(
                &scope_handle_for_timer,
                SCOPE_IMAGE_WIDTH,
                SCOPE_IMAGE_HEIGHT,
            );
            ui.set_scope_waveform(Image::from_rgb8(buffer));
        }
    });

    ui.run().context("failed to run UI")
}

/// Update the UI's visual connection list
fn update_ui_connections(ui: &ModularWindow, connections: &Arc<Mutex<Vec<ConnectionState>>>) {
    if let Ok(conns) = connections.lock() {
        println!("Updating UI with {} connections", conns.len());

        // Convert to Slint's Connection struct
        // Positions are computed reactively in Slint from node positions
        let slint_conns: Vec<_> = conns
            .iter()
            .map(|c| Connection {
                from: c.from,
                to: c.to,
            })
            .collect();

        let model = std::rc::Rc::new(slint::VecModel::from(slint_conns));
        ui.set_connections(model.into());
    }
}

/// Calculate jack positions based on node positions and dimensions
fn update_jack_positions(ui: &ModularWindow) {
    // Node dimensions from Slint (these should match the Dimensions global)
    const NODE_CONTENT_PADDING: f32 = 10.0;
    const JACK_WIDTH: f32 = 30.0;
    const JACK_H_PADDING: f32 = 5.0;
    const JACK_CIRCLE_SIZE: f32 = 20.0;
    const JACK_CENTER_OFFSET: f32 = 12.0;
    const KNOB_AREA_Y: f32 = 32.0;
    const KNOB_HEIGHT: f32 = 55.0;
    const KNOBS_BOTTOM_PADDING: f32 = 2.0;
    const JACK_SPACING: f32 = 2.0;

    // Helper to calculate jack Y position based on knob count
    let calc_jack_y = |knob_count: usize, extra_height: f32| -> f32 {
        let knob_rows = if knob_count == 0 {
            0
        } else {
            (knob_count - 1) / 3 + 1
        };
        let knobs_area_height = (knob_rows as f32) * KNOB_HEIGHT;
        KNOB_AREA_Y + knobs_area_height + KNOBS_BOTTOM_PADDING + JACK_SPACING + extra_height
            + JACK_CENTER_OFFSET
    };

    // Helper to calculate output jack X position (right side of node) based on node width
    let calc_output_jack_x = |node_width: f32| -> f32 {
        node_width - NODE_CONTENT_PADDING - JACK_WIDTH + JACK_H_PADDING + JACK_CIRCLE_SIZE / 2.0
    };

    // Input jack X (left side of node) - same for all nodes
    let input_jack_x = NODE_CONTENT_PADDING + JACK_H_PADDING + JACK_CIRCLE_SIZE / 2.0;

    // Get node positions from Slint
    let sine_x = ui.get_sine_x();
    let sine_y = ui.get_sine_y();
    let saw_x = ui.get_saw_x();
    let saw_y = ui.get_saw_y();
    let filter_x = ui.get_filter_x();
    let filter_y = ui.get_filter_y();
    let scope_x = ui.get_scope_x();
    let scope_y = ui.get_scope_y();
    let output_x = ui.get_output_x();
    let output_y = ui.get_output_y();

    // Get node widths from Slint
    let sine_width = ui.get_sine_width();
    let saw_width = ui.get_saw_width();
    let filter_width = ui.get_filter_width();
    let scope_width = ui.get_scope_width();

    let jack_positions = vec![
        // Sine output (id: 0, 1 knob)
        JackPositionInfo {
            id: 0,
            x: sine_x + calc_output_jack_x(sine_width),
            y: sine_y + calc_jack_y(1, 0.0),
            is_input: false,
        },
        // Saw output (id: 1, 1 knob)
        JackPositionInfo {
            id: 1,
            x: saw_x + calc_output_jack_x(saw_width),
            y: saw_y + calc_jack_y(1, 0.0),
            is_input: false,
        },
        // Filter input (id: 2, 2 knobs)
        JackPositionInfo {
            id: 2,
            x: filter_x + input_jack_x,
            y: filter_y + calc_jack_y(2, 0.0),
            is_input: true,
        },
        // Filter output (id: 3, 2 knobs)
        JackPositionInfo {
            id: 3,
            x: filter_x + calc_output_jack_x(filter_width),
            y: filter_y + calc_jack_y(2, 0.0),
            is_input: false,
        },
        // Output input (id: 4, 1 knob)
        JackPositionInfo {
            id: 4,
            x: output_x + input_jack_x,
            y: output_y + calc_jack_y(1, 0.0),
            is_input: true,
        },
        // Scope input (id: 5, 0 knobs, 120px extra height)
        JackPositionInfo {
            id: 5,
            x: scope_x + input_jack_x,
            y: scope_y + calc_jack_y(0, 120.0),
            is_input: true,
        },
        // Scope output (id: 6, 0 knobs, 120px extra height)
        JackPositionInfo {
            id: 6,
            x: scope_x + calc_output_jack_x(scope_width),
            y: scope_y + calc_jack_y(0, 120.0),
            is_input: false,
        },
    ];

    // Convert to Slint's JackInfo struct
    let slint_jack_infos: Vec<_> = jack_positions
        .into_iter()
        .map(|jp| JackInfo {
            id: jp.id,
            x: jp.x,
            y: jp.y,
            is_input: jp.is_input,
        })
        .collect();

    let model = std::rc::Rc::new(slint::VecModel::from(slint_jack_infos));
    ui.set_jack_registry(model.into());
}
