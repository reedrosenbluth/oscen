use crate::graph::{
    EndpointType, InputEndpoint, NodeKey, OutputEndpoint, ProcessingContext, ProcessingNode,
    SignalProcessor, ValueKey,
};
use crate::ring_buffer::RingBuffer;
use oscen_macros::Node;

#[derive(Debug, Node)]
pub struct Delay {
    #[input(stream)]
    input: f32,
    #[input]
    delay_time: f32, // In seconds
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
    pub fn new(delay_time: f32, feedback: f32) -> Self {
        // Use a small buffer size initially to prevent stack overflow during initialization
        // The real buffer will be properly allocated in init() with the correct sample rate
        let default_sample_rate = 44100.0;

        // Start with a very small buffer to avoid excessive stack usage
        let initial_buffer_size = 1024;

        Self {
            input: 0.0,
            delay_time,
            feedback,
            output: 0.0,
            buffer: RingBuffer::new(initial_buffer_size),
            sample_rate: default_sample_rate,
            frames_per_update: 32,
            frame_counter: 0,
        }
    }

    fn apply_parameter_updates(&mut self, delay_time: f32, feedback: f32) {
        if self.frame_counter == 0 {
            self.delay_time = delay_time.clamp(0.0, 2.0);
            self.feedback = feedback.clamp(0.0, 0.99);
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }

    fn process_sample(&mut self, sample_rate: f32, input: f32) -> f32 {
        let delay_samples = self.delay_time * sample_rate;
        let max_delay = self.buffer.capacity() as f32 - 1.0;
        let clamped_delay = delay_samples.min(max_delay).max(0.0);

        let delayed = self.buffer.get(clamped_delay);
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
        let delay_time = self.get_delay_time(context);
        let feedback = self.get_feedback(context);

        self.apply_parameter_updates(delay_time, feedback);
        self.process_sample(sample_rate, input)
    }
}
