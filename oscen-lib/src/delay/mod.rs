use crate::graph::{
    EndpointDefinition, EndpointMetadata, InputEndpoint, NodeKey, OutputEndpoint, ProcessingNode,
    SignalProcessor, ValueKey,
};
use crate::ring_buffer::RingBuffer;
use oscen_macros::Node;

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

    buffer: RingBuffer<88200>,
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
        let delay_samples = self.delay_time * sample_rate;

        // Read delayed sample using get() method
        let delayed = self.buffer.get(delay_samples);

        // Write input + feedback to buffer using push() method
        self.buffer.push(input + delayed * self.feedback);

        self.output = delayed;
        self.output
    }
}