use oscen::prelude::*;

// Test connecting node event output to graph event output
graph! {
    name: EventPassthroughGraph;
    compile_time: true;

    input midi_in: event;
    output note_on_out: event;
    output note_off_out: event;

    nodes {
        midi_parser = MidiParser::new();
    }

    connections {
        midi_in -> midi_parser.midi_in;
        midi_parser.note_on -> note_on_out;
        midi_parser.note_off -> note_off_out;
    }
}

fn main() {
    let mut graph = EventPassthroughGraph::new(48000.0);
    graph.process();
    println!("Success!");
}
