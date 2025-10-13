use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor,
    ValueKey,
};
use crate::ring_buffer::RingBuffer;
use oscen_macros::Node;

#[derive(Debug, Node)]
pub struct Delay {
    #[input(stream)]
    input: f32,
    #[input]
    delay_samples: f32,
    #[input]
    feedback: f32,

    #[output(stream)]
    output: f32,

    buffer: RingBuffer,
    sample_rate: f32,
    frames_per_update: usize,
    frame_counter: usize,
}

impl Delay {
    /// Create a delay with delay time specified in samples/frames.
    pub fn new(delay_samples: f32, feedback: f32) -> Self {
        // Start with a very small buffer to avoid excessive stack usage
        let initial_buffer_size = 1024;

        Self {
            input: 0.0,
            delay_samples,
            feedback,
            output: 0.0,
            buffer: RingBuffer::new(initial_buffer_size),
            sample_rate: 44100.0, // Default, will be overwritten in init()
            frames_per_update: 32,
            frame_counter: 0,
        }
    }

    /// Create a delay with delay time specified in seconds at a given sample rate.
    pub fn from_seconds(delay_seconds: f32, feedback: f32, sample_rate: f32) -> Self {
        let delay_samples = delay_seconds * sample_rate;
        Self::new(delay_samples, feedback)
    }

    fn apply_parameter_updates(&mut self, delay_samples: f32, feedback: f32) {
        if self.frame_counter == 0 {
            let max_delay = self.buffer.capacity() as f32 - 1.0;
            self.delay_samples = delay_samples.clamp(0.0, max_delay);
            self.feedback = feedback.clamp(0.0, 0.99);
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }

    fn process_sample(&mut self, _sample_rate: f32, input: f32) -> f32 {
        let delayed = self.buffer.get(self.delay_samples);
        self.buffer.push(input + delayed * self.feedback);

        self.output = delayed;
        self.output
    }
}

impl SignalProcessor for Delay {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Calculate a reasonable buffer size based on sample rate, with a safety cap
        // to prevent potential stack overflows
        let target_seconds = 2.0;
        let max_samples = 88200; // Maximum buffer size (2 seconds at 44.1kHz)

        let buffer_size = ((target_seconds * sample_rate) as usize).min(max_samples);

        // Initialize the buffer with a capped size
        self.buffer = RingBuffer::new(buffer_size);
    }

    fn allows_feedback(&self) -> bool {
        true // Delay nodes can break feedback cycles
    }

    fn process<'a>(&mut self, sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let input = self.get_input(context);
        let delay_samples = self.get_delay_samples(context);
        let feedback = self.get_feedback(context);

        self.apply_parameter_updates(delay_samples, feedback);
        self.process_sample(sample_rate, input)
    }
}
