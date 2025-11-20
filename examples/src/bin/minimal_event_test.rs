use oscen::prelude::*;

// Minimal test case
graph! {
    name: MinimalEventGraph;
    compile_time: true;

    input midi_in: event;
    output note_on_out: event;

    nodes {
        midi_parser = MidiParser::new();
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> note_on_out;
    }
}

fn main() {
    let mut graph = MinimalEventGraph::new(48000.0);
    graph.process();
    println!("Success!");
}
