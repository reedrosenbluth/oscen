# Swell

Swell is a library for building modular synthesizers in Rust.

It contains a collection of components frequently used in sound synthesis
such as oscillators, filters, and envelope generators. It lets you
connect (or patch) the output of one module into the input of another.

## Example

```Rust
fn build_synth(midi_pitch: ArcMutex<MidiPitch>) -> Graph {
    // Oscillator 1
    let sine1 = SineOsc::with_hz(cv("modulator_osc1"));
    let saw1 = SawOsc::with_hz(cv("midi_pitch"));
    let square1 = SquareOsc::with_hz(cv("midi_pitch"));
    let triangle1 = TriangleOsc::with_hz(cv("midi_pitch"));

    let modulator_osc1 = Modulator::wrapped("sine2", cv("midi_pitch"), fix(0.0), fix(0.0));

    // Oscillator 2
    let sine2 = SineOsc::with_hz(cv("modulator_osc2"));
    let saw2 = SawOsc::with_hz(cv("midi_pitch"));
    let square2 = SquareOsc::with_hz(cv("midi_pitch"));
    let triangle2 = TriangleOsc::with_hz(cv("midi_pitch"));

    let modulator_osc2 = Modulator::wrapped("tri_lfo", cv("midi_pitch"), fix(0.0), fix(0.0));

    // LFO
    let tri_lfo = TriangleOsc::wrapped();
    let square_lfo = SquareOsc::wrapped();

    // Noise
    let noise = WhiteNoise::wrapped();

    // Mixers
    // sine1 + saw1
    let mixer1 = Mixer::wrapped(vec!["sine1", "saw1"]);
    // square1 + sub1
    let mixer2 = Mixer::wrapped(vec!["square1", "sub1"]);
    // mixer1 + mixer2
    let mixer3 = Mixer::wrapped(vec!["mixer1", "mixer2"]);

    // Envelope Generator
    let adsr = SustainSynth::wrapped("mixer3");

    Graph::new(vec![("midi_pitch", midi_pitch),
                        ("sine1", arc(sine1)),
                        ("saw1", arc(saw1)),
                        ("square1", arc(square1)),
                        ("triangle1", arc(triangle1)),
                        ("sine2", arc(sine2)),
                        ("saw2", arc(saw2)),
                        ("square2", arc(square2)),
                        ("triangle2", arc(triangle2)),
                        ("modulator_osc1", modulator_osc1),
                        ("modulator_osc2", modulator_osc2),
                        ("noise", noise),
                        ("tri_lfo", tri_lfo),
                        ("square_lfo", square_lfo),
                        ("mixer1", mixer1),
                        ("mixer2", mixer2),
                        ("mixer3", mixer3),
                        ("adsr", adsr),
                       ])
}
```
