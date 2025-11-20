use oscen::graph;

// Mock node for testing
#[derive(Clone, Copy, Debug)]
pub struct MockVoice {
    pub brightness: f32,
    pub gate: bool,
}

impl MockVoice {
    pub fn new(_sample_rate: f32) -> Self {
        Self {
            brightness: 0.0,
            gate: false,
        }
    }
    pub fn process(&mut self) {}
}

graph! {
    name: StaticGraphTest;
    compile_time: true;

    input value brightness = 30.0;
    // input event gate; // Skipping EventParam for now as it requires more setup

    nodes {
        // Test array initialization with simplified DSL (implicit new(sample_rate))
        voices = [MockVoice; 4];
        // Test single node initialization with simplified DSL
        single = MockVoice;
    }

    connections {
        // Test broadcasting input to array
        brightness -> voices.brightness;
        // Test scalar to scalar
        brightness -> single.brightness;
    }
}

fn main() {
    let mut graph = StaticGraphTest::new(44100.0);
    
    // Test if inputs are generated as fields
    graph.brightness = 0.5;
    
    // Run process to trigger connection assignments
    graph.process();
    
    // Verify values
    println!("Voice 0 brightness: {}", graph.voices[0].brightness);
    println!("Single brightness: {}", graph.single.brightness);
    
    assert_eq!(graph.voices[0].brightness, 0.5);
    assert_eq!(graph.single.brightness, 0.5);
    
    println!("Test passed!");
}
