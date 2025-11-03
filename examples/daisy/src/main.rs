#![no_std]
#![no_main]

use panic_halt as _;

use cortex_m::asm;
use cortex_m_rt::entry;

use daisy_bsp::hal;
use daisy_bsp::led::Led;
use daisy_bsp::loggit;
use daisy_bsp::prelude::*;

use oscen::{Graph, Oscillator, StreamOutput, TptFilter};

// Audio sample rate - Daisy typically runs at 48kHz
const SAMPLE_RATE: f32 = 48000.0;

// Global audio graph state
// In a real application, you'd want more sophisticated state management,
// but for this example we'll keep it simple
static mut AUDIO_GRAPH: Option<Graph> = None;
static mut OUTPUT_NODE: Option<StreamOutput> = None;

#[entry]
fn main() -> ! {
    loggit!("Oscen Daisy Example Starting...");

    // Initialize the Daisy board
    let board = daisy_bsp::Board::take().unwrap();
    let mut led = board.leds.USER;
    led.set_high();

    loggit!("Board initialized");

    // Create the audio graph
    let mut graph = Graph::new(SAMPLE_RATE);

    // Create a simple patch: Sine oscillator -> Low-pass filter
    // The oscillator runs at 220Hz (A3) with 0.3 amplitude
    let osc = graph.add_node(Oscillator::sine(220.0, 0.3));

    // Filter with 1200Hz cutoff and moderate resonance (Q = 0.707)
    let filter = graph.add_node(TptFilter::new(1200.0, 0.707));

    // Connect oscillator to filter
    graph.connect(osc.output, filter.input);

    // Store the graph and output node in static variables
    // so the audio callback can access them
    unsafe {
        OUTPUT_NODE = Some(filter.output);
        AUDIO_GRAPH = Some(graph);
    }

    loggit!("Audio graph created");

    // Start the audio processing
    // The audio callback will be called automatically by the BSP
    board
        .audio
        .start(|_fs, block| {
            // Process audio block
            for frame in block {
                // Process one sample through the graph
                unsafe {
                    if let Some(ref mut graph) = AUDIO_GRAPH {
                        if let Err(_e) = graph.process() {
                            // In a real application, you'd want to handle this error
                            // but in no_std we can't easily log it
                        }

                        // Get the output value
                        if let Some(ref output) = OUTPUT_NODE {
                            if let Some(value) = graph.get_value(output) {
                                // Write to both left and right channels
                                frame[0] = value;
                                frame[1] = value;
                            }
                        }
                    }
                }
            }
        })
        .unwrap();

    loggit!("Audio started - you should hear a 220Hz tone!");

    // Blink LED to show we're running
    let mut counter = 0u32;
    loop {
        asm::delay(480_000_000); // Roughly 1 second delay

        counter = counter.wrapping_add(1);
        if counter % 2 == 0 {
            led.set_high();
        } else {
            led.set_low();
        }
    }
}
