use crate::{InputEndpoint, Node, NodeKey, ProcessingNode, SignalProcessor, ValueKey};

/// Test node with multi-channel stream input
#[derive(Debug, Node)]
pub struct MultiChannelReceiver {
    #[input(stream)]
    pub inputs: [f32; 4],

    #[output(stream)]
    pub sum: f32,
}

impl MultiChannelReceiver {
    pub fn new() -> Self {
        Self {
            inputs: [0.0; 4],
            sum: 0.0,
        }
    }
}

impl SignalProcessor for MultiChannelReceiver {
    fn process(&mut self) {
        self.sum = self.inputs.iter().sum();
    }
}

/// Test node with multi-channel stream output
#[derive(Debug, Node)]
pub struct MultiChannelSource {
    #[output(stream)]
    pub outputs: [f32; 4],

    value: f32,
}

impl MultiChannelSource {
    pub fn new(value: f32) -> Self {
        Self {
            outputs: [0.0; 4],
            value,
        }
    }
}

impl SignalProcessor for MultiChannelSource {
    fn process(&mut self) {
        for i in 0..4 {
            self.outputs[i] = self.value * (i + 1) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_channel_source_generates_array() {
        let mut source = MultiChannelSource::new(10.0);
        let sample_rate = 48_000.0;

        source.init(sample_rate);
        source.process();

        assert_eq!(source.outputs[0], 10.0);
        assert_eq!(source.outputs[1], 20.0);
        assert_eq!(source.outputs[2], 30.0);
        assert_eq!(source.outputs[3], 40.0);
    }

    #[test]
    fn test_multi_channel_receiver_sums_channels() {
        let mut receiver = MultiChannelReceiver::new();
        receiver.inputs = [1.0, 2.0, 3.0, 4.0];

        let _sample_rate = 48_000.0;
        receiver.process();

        assert_eq!(receiver.sum, 10.0);
    }

    #[test]
    fn test_multi_channel_direct_field_access() {
        let mut source = MultiChannelSource::new(1.0);
        source.process();

        // Verify fields are directly accessible (static graph pattern)
        assert_eq!(source.outputs[0], 1.0);
        assert_eq!(source.outputs[1], 2.0);
        assert_eq!(source.outputs[2], 3.0);
        assert_eq!(source.outputs[3], 4.0);
    }

    #[test]
    fn test_multi_channel_receiver_direct_field_access() {
        let mut receiver = MultiChannelReceiver::new();

        // Set inputs directly (static graph pattern)
        receiver.inputs = [7.0, 8.0, 9.0, 10.0];

        // Process and verify sum
        receiver.process();
        assert_eq!(receiver.sum, 34.0); // 7 + 8 + 9 + 10
    }
}
