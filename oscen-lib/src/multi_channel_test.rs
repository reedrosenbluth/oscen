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
        source.process(sample_rate);

        assert_eq!(source.outputs[0], 10.0);
        assert_eq!(source.outputs[1], 20.0);
        assert_eq!(source.outputs[2], 30.0);
        assert_eq!(source.outputs[3], 40.0);
    }

    #[test]
    fn test_multi_channel_receiver_sums_channels() {
        let mut receiver = MultiChannelReceiver::new();
        receiver.inputs = [1.0, 2.0, 3.0, 4.0];

        let sample_rate = 48_000.0;
        receiver.process(sample_rate);

        assert_eq!(receiver.sum, 10.0);
    }

    #[test]
    fn test_node_io_get_stream_output_channels() {
        use crate::graph::NodeIO;

        let mut source = MultiChannelSource::new(1.0);
        source.process(48_000.0);

        // Test get_stream_output_channels via NodeIO trait
        let channels = source.get_stream_output_channels(0);
        assert_eq!(channels.len(), 4);
        assert_eq!(channels[0], 1.0);
        assert_eq!(channels[1], 2.0);
        assert_eq!(channels[2], 3.0);
        assert_eq!(channels[3], 4.0);

        // Verify fields are directly accessible
        assert_eq!(source.outputs[0], 1.0);
        assert_eq!(source.outputs[1], 2.0);
        assert_eq!(source.outputs[2], 3.0);
        assert_eq!(source.outputs[3], 4.0);
    }

    #[test]
    fn test_node_io_set_stream_input_channels() {
        use crate::graph::NodeIO;

        let mut receiver = MultiChannelReceiver::new();
        let test_channels = [7.0, 8.0, 9.0, 10.0];
        receiver.set_stream_input_channels(0, &test_channels);

        // Verify the array was copied via NodeIO
        assert_eq!(receiver.inputs[0], 7.0);
        assert_eq!(receiver.inputs[1], 8.0);
        assert_eq!(receiver.inputs[2], 9.0);
        assert_eq!(receiver.inputs[3], 10.0);

        // Process and verify sum
        receiver.process(48_000.0);
        assert_eq!(receiver.sum, 34.0); // 7 + 8 + 9 + 10
    }
}
