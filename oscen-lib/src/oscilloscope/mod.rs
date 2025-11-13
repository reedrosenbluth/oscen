use std::sync::{
    atomic::{AtomicU32, AtomicUsize, Ordering},
    Arc,
};

use crate::graph::{
    InputEndpoint, NodeKey, ProcessingContext, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::Node;

#[derive(Debug)]
struct SharedState {
    samples: Vec<AtomicU32>,
    write_counter: AtomicUsize,
    samples_written: AtomicUsize,
    capacity: usize,
    triggered_samples: Vec<AtomicU32>,
    triggered_length: AtomicUsize,
}

impl SharedState {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            write_counter: AtomicUsize::new(0),
            samples_written: AtomicUsize::new(0),
            capacity,
            triggered_samples: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            triggered_length: AtomicUsize::new(0),
        }
    }
}

#[derive(Clone)]
pub struct OscilloscopeHandle {
    state: Arc<SharedState>,
}

impl OscilloscopeHandle {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Arc::new(SharedState::new(capacity)),
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.state.capacity
    }

    #[inline]
    pub fn push(&self, sample: f32) {
        let counter = self.state.write_counter.fetch_add(1, Ordering::Relaxed);
        let idx = counter % self.state.capacity;
        self.state.samples[idx].store(sample.to_bits(), Ordering::Relaxed);

        let written = self.state.samples_written.load(Ordering::Relaxed);
        if written < self.state.capacity {
            let _ = self.state.samples_written.compare_exchange(
                written,
                written + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    pub fn snapshot(&self, sample_count: usize) -> OscilloscopeSnapshot {
        let available = self
            .state
            .samples_written
            .load(Ordering::Acquire)
            .min(self.state.capacity);
        if available == 0 {
            return OscilloscopeSnapshot {
                samples: vec![],
                triggered: vec![],
            };
        }

        let requested = sample_count.max(1).min(available);
        let mut dest = Vec::with_capacity(requested);
        let counter = self.state.write_counter.load(Ordering::Acquire);

        for offset in (0..requested).rev() {
            let idx = counter
                .wrapping_sub(1 + offset)
                .rem_euclid(self.state.capacity);
            let bits = self.state.samples[idx].load(Ordering::Relaxed);
            dest.push(f32::from_bits(bits));
        }

        let trigger_len = self
            .state
            .triggered_length
            .load(Ordering::Acquire)
            .min(self.state.capacity);
        let mut triggered = Vec::with_capacity(trigger_len);
        if trigger_len > 0 {
            for i in 0..trigger_len {
                let bits = self.state.triggered_samples[i].load(Ordering::Relaxed);
                triggered.push(f32::from_bits(bits));
            }
        }

        OscilloscopeSnapshot {
            samples: dest,
            triggered,
        }
    }

    fn store_triggered(&self, length: usize) {
        let length = length.min(self.state.capacity).max(1);
        let counter = self.state.write_counter.load(Ordering::Acquire);
        for offset in (0..length).rev() {
            let idx = counter
                .wrapping_sub(1 + offset)
                .rem_euclid(self.state.capacity);
            let value = self.state.samples[idx].load(Ordering::Relaxed);
            let target = length - 1 - offset;
            self.state.triggered_samples[target].store(value, Ordering::Relaxed);
        }
        self.state.triggered_length.store(length, Ordering::Release);
    }

    pub fn clear_triggered(&self) {
        self.state.triggered_length.store(0, Ordering::Release);
    }
}

impl std::fmt::Debug for OscilloscopeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OscilloscopeHandle")
            .field("capacity", &self.capacity())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct OscilloscopeSnapshot {
    samples: Vec<f32>,
    triggered: Vec<f32>,
}

impl OscilloscopeSnapshot {
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn triggered(&self) -> &[f32] {
        &self.triggered
    }
}

pub const DEFAULT_SCOPE_CAPACITY: usize = 4096;

#[derive(Debug, Node)]
pub struct Oscilloscope {
    #[input(stream)]
    input: f32,

    #[output(stream)]
    output: f32,

    #[output(value)]
    handle: OscilloscopeHandle,

    #[input(value)]
    trigger_period: f32,

    #[input(value)]
    trigger_enabled: f32,

    last_sample: f32,
    auto_detect_period: bool,
    period_sample_count: usize,
    detected_period: usize,
    io: OscilloscopeIO,
}

impl Oscilloscope {
    pub fn new(capacity: usize) -> Self {
        let handle = OscilloscopeHandle::new(capacity);
        Self {
            input: 0.0,
            output: 0.0,
            handle,
            trigger_period: capacity as f32,
            trigger_enabled: 1.0,
            last_sample: 0.0,
            auto_detect_period: false,
            period_sample_count: 0,
            detected_period: capacity,
            io: OscilloscopeIO {
                input: 0.0,
                output: 0.0,
            },
        }
    }

    pub fn with_handle(handle: OscilloscopeHandle) -> Self {
        let capacity = handle.capacity();
        Self {
            input: 0.0,
            output: 0.0,
            handle,
            trigger_period: capacity as f32,
            trigger_enabled: 1.0,
            last_sample: 0.0,
            auto_detect_period: false,
            period_sample_count: 0,
            detected_period: capacity,
            io: OscilloscopeIO {
                input: 0.0,
                output: 0.0,
            },
        }
    }

    pub fn with_auto_detect(handle: OscilloscopeHandle) -> Self {
        let capacity = handle.capacity();
        Self {
            input: 0.0,
            output: 0.0,
            handle,
            trigger_period: capacity as f32,
            trigger_enabled: 1.0,
            last_sample: 0.0,
            auto_detect_period: true,
            period_sample_count: 0,
            detected_period: capacity,
            io: OscilloscopeIO {
                input: 0.0,
                output: 0.0,
            },
        }
    }

    pub fn handle(&self) -> &OscilloscopeHandle {
        &self.handle
    }
}

impl Default for Oscilloscope {
    fn default() -> Self {
        Self::new(DEFAULT_SCOPE_CAPACITY)
    }
}

impl SignalProcessor for Oscilloscope {
    /// Process using struct-of-arrays I/O pattern.
    ///
    /// Input and output are accessed via self.input/self.output
    /// Graph pre-populates input, node writes to output.
    fn process<'a>(
        &mut self,
        _sample_rate: f32,
        context: &mut ProcessingContext<'a>,
    ) {
        // Read stream input from self (pre-populated by graph)
        let input = self.input;

        // Pass through input to output
        self.output = input;
        self.handle.push(input);

        if self.get_trigger_enabled(context) > 0.5 {
            let prev = self.last_sample;
            let zero_crossing = prev <= 0.0 && input > 0.0;

            if self.auto_detect_period {
                // Auto-detect period by counting samples between zero crossings
                self.period_sample_count += 1;

                if zero_crossing {
                    // Found a zero crossing - use the counted samples as the period
                    if self.period_sample_count > 1 {
                        let capacity = self.handle.capacity();
                        // Clamp detected period to reasonable bounds
                        self.detected_period = self.period_sample_count.min(capacity).max(10);
                    }

                    // Store triggered buffer with detected period
                    if self.detected_period > 0 {
                        self.handle.store_triggered(self.detected_period);
                    }

                    // Reset counter for next period
                    self.period_sample_count = 0;
                }
            } else {
                // Manual mode: use trigger_period parameter
                if zero_crossing {
                    let period = self.get_trigger_period(context).max(1.0);
                    let length = period.round() as usize;
                    if length > 0 {
                        self.handle.store_triggered(length);
                    }
                }
            }
        }

        self.last_sample = input;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_returns_recent_samples_in_order() {
        let handle = OscilloscopeHandle::new(8);
        for i in 0..10 {
            handle.push(i as f32);
        }

        let snapshot = handle.snapshot(4);
        assert_eq!(snapshot.samples(), &[6.0, 7.0, 8.0, 9.0]);
        assert!(snapshot.triggered().is_empty());
    }

    #[test]
    fn snapshot_gracefully_handles_empty_buffer() {
        let handle = OscilloscopeHandle::new(16);
        let snapshot = handle.snapshot(8);
        assert!(snapshot.samples().is_empty());
        assert!(snapshot.triggered().is_empty());
    }

    #[test]
    fn snapshot_largest_available_when_request_exceeds_capacity() {
        let handle = OscilloscopeHandle::new(4);
        for i in 0..4 {
            handle.push(i as f32);
        }

        let snapshot = handle.snapshot(16);
        assert_eq!(snapshot.samples(), &[0.0, 1.0, 2.0, 3.0]);
        assert!(snapshot.triggered().is_empty());
    }
}
