pub mod graph;

pub use graph::*;

pub use oscen2_macros::Node;
use std::f32::consts::PI;

#[derive(Debug, Node)]
pub struct Oscillator {
    #[input]
    phase: f32,
    #[input]
    frequency: f32,
    #[input]
    frequency_mod: f32,
    #[input]
    amplitude: f32,

    #[output]
    output: f32,

    waveform: fn(f32) -> f32,
}

impl Oscillator {
    pub fn new(frequency: f32, amplitude: f32, waveform: fn(f32) -> f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            frequency_mod: 0.0,
            amplitude,
            waveform,
            output: 0.0,
        }
    }

    pub fn sine(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| (p * 2.0 * PI).sin())
    }

    pub fn square(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| if p < 0.5 { 1.0 } else { -1.0 })
    }

    // Anti-aliased sawtooth using polynomial transition region
    pub fn saw(frequency: f32, amplitude: f32) -> Self {
        Self::new(frequency, amplitude, |p| {
            // Map phase from [0,1] to [-1,1]
            let x = p * 2.0 - 1.0;

            // Width of transition region (adjust for aliasing vs sharpness tradeoff)
            let transition_width = 0.1;

            // Linear ramp from -1 to 1
            let raw_saw = x;

            // Smooth transition near discontinuity using polynomial
            if x > (1.0 - transition_width) {
                let t = (x - (1.0 - transition_width)) / transition_width;
                let smoothed = -1.0 + (1.0 - t * t) * (raw_saw + 1.0);
                smoothed
            } else {
                raw_saw
            }
        })
    }
}

impl SignalProcessor for Oscillator {
    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let phase_mod = self.get_phase(inputs);
        let freq_mod = self.get_frequency_mod(inputs);
        let freq_offset = self.get_frequency(inputs);
        let amp_mod = self.get_amplitude(inputs);

        // Use the initial frequency value when no input is connected
        let base_freq = if freq_offset == 0.0 {
            self.frequency
        } else {
            freq_offset
        };
        let frequency = base_freq * (1.0 + freq_mod);
        let amplitude = self.amplitude * (1.0 + amp_mod);

        let modulated_phase = (self.phase + phase_mod) % 1.0;
        self.output = (self.waveform)(modulated_phase) * amplitude;

        self.phase += frequency / sample_rate;
        self.phase %= 1.0; // Keep phase between 0 and 1

        self.output
    }
}

#[derive(Debug, Default, Node)]
pub struct TPT_Filter {
    #[input]
    input: f32,
    #[input]
    cutoff: f32,
    #[input]
    q: f32,

    #[output]
    output: f32,

    // state
    z: [f32; 2],

    // coefficients
    d: f32,
    a: f32,
    g1: f32,

    // frame counting
    frame_counter: usize,
    frames_per_update: usize,
}

/// These filters are based on the designs outlined in The Art of VA Filter Design
/// by Vadim Zavalishin, with help from Will Pirkle in Virtual Analog Filter Implementation.
/// The topology-preserving transform approach leads to designs where parameter
/// modulation can be applied with minimal instability.
///
/// Parameter changes are applied at a lower rate than processor.frequency to reduce
/// computational cost, and the frames between updates can be altered using the
/// `framesPerParameterUpdate`, smaller numbers causing more frequent updates.
impl TPT_Filter {
    pub fn new(cutoff: f32, q: f32) -> Self {
        Self {
            cutoff,
            q,
            frames_per_update: 32,
            ..Default::default()
        }
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        let freq = self.cutoff.clamp(20.0, sample_rate * 0.48);
        let period = 0.5 / sample_rate;
        let f = (2.0 * sample_rate) * (2.0 * PI * freq * period).tan() * period;
        let inv_q = 1.0 / self.q;

        self.d = 1.0 / (1.0 + inv_q * f + f * f);
        self.a = f;
        self.g1 = f + inv_q;
    }
}

impl SignalProcessor for TPT_Filter {
    fn init(&mut self, sample_rate: f32) {
        self.update_coefficients(sample_rate);
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let input = self.get_input(inputs);

        if self.frame_counter == 0 {
            let cutoff = self.get_cutoff(inputs).clamp(20.0, sample_rate * 0.5);
            let q = self.get_q(inputs).clamp(0.1, 10.0);

            if cutoff != self.cutoff || q != self.q {
                self.cutoff = cutoff;
                self.q = q;
                self.update_coefficients(sample_rate);
            }
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;

        let high = (input - self.g1 * self.z[0] - self.z[1]) * self.d;
        let band = self.a * high + self.z[0];
        let low = self.a * band + self.z[1];

        self.z[0] = self.a * high + band;
        self.z[1] = self.a * band + low;

        self.output = low;
        self.output
    }
}

#[derive(Debug, Node)]
pub struct Delay {
    #[input]
    input: f32,
    #[input]
    delay_time: f32, // In seconds
    #[input]
    feedback: f32,

    #[output]
    output: f32,

    buffer: RingBuffer,
    sample_rate: f32,
    frames_per_update: usize,
    frame_counter: usize,
}

impl Delay {
    pub fn new(delay_time: f32, feedback: f32) -> Self {
        Self {
            input: 0.0,
            delay_time,
            feedback,
            output: 0.0,
            buffer: RingBuffer::new(88200), // 2 seconds at 44.1kHz
            sample_rate: 44100.0,
            frames_per_update: 32,
            frame_counter: 0,
        }
    }
}

impl SignalProcessor for Delay {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.buffer = RingBuffer::new((2.0 * sample_rate) as usize);
    }

    fn process(&mut self, sample_rate: f32, inputs: &[f32]) -> f32 {
        let input = self.get_input(inputs);

        if self.frame_counter == 0 {
            let delay_time = self.get_delay_time(inputs).clamp(0.0, 2.0);
            let feedback = self.get_feedback(inputs).clamp(0.0, 0.99);

            // Update parameters
            self.delay_time = delay_time;
            self.feedback = feedback;
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;

        // Calculate delay in samples
        let delay_samples = (self.delay_time * sample_rate) as f32;

        // Read delayed sample using get() method
        let delayed = self.buffer.get(delay_samples);

        // Write input + feedback to buffer using push() method
        self.buffer.push(input + delayed * self.feedback);

        self.output = delayed;
        self.output
    }
}

#[test]
fn test_audio_render_fm() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(880.0, 0.5));
    let carrier = graph.add_node(Oscillator::sine(254.37, 0.5));

    graph.connect(modulator.output(), carrier.frequency_mod());

    graph
        .render_to_file(5.0, "test_output_fm.wav")
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_fm2() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(0.5, 0.5));
    let carrier = graph.add_node(Oscillator::sine(440., 0.5));

    graph.connect(modulator.output(), carrier.frequency());

    graph
        .render_to_file(5.0, "test_output_fm2.wav")
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_am() {
    let mut graph = Graph::new(44100.0);

    let lfo = graph.add_node(Oscillator::sine(0.5, 0.5));
    let osc2 = graph.add_node(Oscillator::sine(440., 0.5));

    graph.connect(lfo.output(), osc2.amplitude());

    graph
        .render_to_file(5.0, "test_output_am.wav") // 4 seconds to hear 2 full cycles
        .expect("Failed to render audio");
}

#[test]
fn test_audio_render_debug() {
    let mut graph = Graph::new(44100.0);

    let modulator = graph.add_node(Oscillator::sine(5.0, 100.0));
    let carrier = graph.add_node(Oscillator::sine(440.0, 0.5));

    graph.connect(modulator.output(), carrier.frequency());

    // Process just 10 samples
    for i in 0..10 {
        println!("\nProcessing sample {}", i);
        graph.process();
    }
}

#[test]
fn test_filter_debug() {
    let mut graph = Graph::new(44100.0);

    let carrier = graph.add_node(Oscillator::saw(440.0, 0.5));
    let filter = graph.add_node(TPT_Filter::new(1000.0, 0.707));

    graph.connect(carrier.output(), filter.input());

    // Process just 10 samples
    for i in 0..100 {
        println!("\nProcessing sample {}", i);
        graph.process();
        if let Some(value) = graph.get_value(&filter.output()) {
            println!("Output value: {}", value);
        }
    }
}
