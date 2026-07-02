use crate::frame::AudioFrame;
use crate::graph::{AllowsFeedback, SampleRate, SignalProcessor};
use crate::ring_buffer::RingBuffer;
use oscen_macros::Node;

/// Below this magnitude the recirculating feedback tail hits denormals, which
/// cost ~100× on x86. Same threshold as the halfband IIR's recursive state.
const DENORMAL_THRESHOLD: f32 = 1e-15;

#[derive(Debug, Node)]
pub struct Delay {
    #[input(stream)]
    pub input: f32,
    #[input(value)]
    delay_samples: f32,
    #[input(value)]
    feedback: f32,

    #[output(stream)]
    pub output: f32,

    buffer: RingBuffer,
    sample_rate: SampleRate,
    frames_per_update: usize,
    frame_counter: usize,
}

impl Delay {
    /// Create a delay with delay time specified in samples/frames.
    pub fn new(delay_samples: f32, feedback: f32) -> Self {
        // Placeholder size; `prepare` resizes for the actual sample rate.
        let initial_buffer_size = 1024;

        Self {
            input: 0.0,
            delay_samples,
            feedback,
            output: 0.0,
            buffer: RingBuffer::new(initial_buffer_size),
            sample_rate: SampleRate::default(),
            frames_per_update: 32,
            frame_counter: 0,
        }
    }

    /// Create a delay with delay time specified in seconds at a given sample rate.
    pub fn from_seconds(delay_seconds: f32, feedback: f32, sample_rate: f32) -> Self {
        let delay_samples = delay_seconds * sample_rate;
        Self::new(delay_samples, feedback)
    }

    fn apply_parameter_updates(&mut self) {
        if self.frame_counter == 0 {
            let max_delay = self.buffer.capacity() as f32 - 1.0;
            self.delay_samples = self.delay_samples.clamp(0.0, max_delay);
            self.feedback = self.feedback.clamp(0.0, 0.99);
        }

        self.frame_counter = (self.frame_counter + 1) % self.frames_per_update;
    }
}

impl SignalProcessor for Delay {
    fn prepare(&mut self) {
        // Size the buffer in time so the maximum delay is the same at every
        // sample rate. PowerOfTwo mode (the `new` default) is kept for its
        // mask-based indexing on the audio thread; the rounding up only adds
        // delay headroom.
        let target_seconds = 2.0;
        let buffer_size = (target_seconds * *self.sample_rate) as usize;
        self.buffer = RingBuffer::new(buffer_size);
    }

    #[inline(always)]
    fn process(&mut self) {
        // Update parameters
        self.apply_parameter_updates();

        // Process delay. Flush the recirculated value so a decaying feedback
        // tail snaps to zero instead of ringing through the subnormal range.
        let delayed = self.buffer.get(self.delay_samples);
        self.buffer
            .push((self.input + delayed * self.feedback).flush_denormal(DENORMAL_THRESHOLD));

        // Write output
        self.output = delayed;
    }
}

impl AllowsFeedback for Delay {}

#[cfg(test)]
mod tests;
