use oscen::prelude::*;

// Test array event outputs
graph! {
    name: ArrayEventGraph;

    input note_on: event;
    input note_off: event;

    nodes {
        voice_allocator = VoiceAllocator::<4>;
    }

    connections {
        note_on -> voice_allocator.note_on;
        note_off -> voice_allocator.note_off;
    }
}

fn main() {
    let mut graph = ArrayEventGraph::new();
    graph.init(48000.0);
    graph.process();
    println!("Success!");
}
