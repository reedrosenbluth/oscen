//! Regression tests for codegen correctness fixes. These compile a graph and
//! assert on the generated token stream (or on a specific generated method's
//! body, extracted via `syn`).

use oscen_graph_compiler::compile;
use quote::{quote, ToTokens};

/// Compile a graph and return the generated tokens as a string.
fn compile_to_string(tokens: proc_macro2::TokenStream) -> String {
    match compile(tokens) {
        Ok(ts) => ts.to_string(),
        Err(diags) => panic!(
            "compile failed: {:?}",
            diags
                .items
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
        ),
    }
}

/// Extract the body of an inherent method from the generated code.
fn inherent_method_body(tokens: proc_macro2::TokenStream, method: &str) -> String {
    let file: syn::File = syn::parse2(tokens).expect("generated code parses as a file");
    for item in file.items {
        if let syn::Item::Impl(imp) = item {
            if imp.trait_.is_some() {
                continue;
            }
            for it in imp.items {
                if let syn::ImplItem::Fn(f) = it {
                    if f.sig.ident == method {
                        return f.block.to_token_stream().to_string();
                    }
                }
            }
        }
    }
    panic!("inherent method `{}` not found in generated code", method);
}

// ---------------------------------------------------------------------------
// Literal-left compound source (`0.5 * g.output -> out`)
// ---------------------------------------------------------------------------

#[test]
fn literal_left_compound_source_keeps_node_alive() {
    // Used to panic in debug (IR validator) and silently prune `g` in
    // release, because `primary_node` only descended the left operand.
    let tokens = compile(quote! {
        name: LitLeftCode;
        input stream s;
        output stream out;
        node g = Gain::new(0.5);
        connections {
            s -> g.input;
            0.5 * g.output -> out;
        }
    })
    .expect("compile succeeds")
    .to_string();
    assert!(
        tokens.contains("pub g :"),
        "node `g` must not be pruned; got no `g` field"
    );
    assert!(
        tokens.contains("self . g . output"),
        "output assignment should read g.output"
    );
}

// ---------------------------------------------------------------------------
// Dead-node pass and asset bindings
// ---------------------------------------------------------------------------

#[test]
fn asset_bound_node_survives_dead_node_pass() {
    // `player` has no path to a graph output; removing it used to leave the
    // AssetBinding pointing at a freed key, panicking in codegen
    // ("invalid SlotMap key used").
    let tokens = compile_to_string(quote! {
        name: AssetKeep;
        input stream s;
        output stream out;
        external sample: AudioAsset;
        node player = SamplePlayer::new();
        node g = Gain::new(0.5);
        connections {
            sample -> player.buf;
            s -> g.input;
            g.output -> out;
        }
    });
    assert!(
        tokens.contains("pub player :"),
        "asset-bound node `player` must stay alive"
    );
    assert!(
        tokens.contains("pub sample :"),
        "asset load handle field `sample` must be generated"
    );
}

// ---------------------------------------------------------------------------
// Ramped value inputs in compound expressions
// ---------------------------------------------------------------------------

#[test]
fn ramped_input_in_compound_expression_reads_current() {
    let tokens = compile_to_string(quote! {
        name: RampCompound;
        input stream s;
        input value gain = 1.0 [ramp: 64];
        output stream out;
        node osc = Gain::new(0.5);
        connections {
            s -> osc.input;
            osc.output * gain -> out;
        }
    });
    assert!(
        tokens.contains("self . osc . output * self . gain . current"),
        "compound expression must read `.current` of the ramped input; got:\n{}",
        tokens
    );
}

#[test]
fn bare_ramped_input_to_value_output_reads_current() {
    let tokens = compile_to_string(quote! {
        name: RampBare;
        input value gain = 1.0 [ramp: 64];
        output value level;
        connections {
            gain -> level;
        }
    });
    assert!(
        tokens.contains("self . level = self . gain . current"),
        "bare ramped input forwarded to a value output must read `.current`; got:\n{}",
        tokens
    );
}

// ---------------------------------------------------------------------------
// Graph event outputs: cleared at cycle start, readable after process()
// ---------------------------------------------------------------------------

#[test]
fn event_output_is_not_cleared_after_population() {
    let tokens = compile(quote! {
        name: EvOut;
        input event midi;
        output event thru;
        node seq = Sequencer::new();
        connections {
            midi -> seq.midi_in;
            seq.midi_out -> thru;
        }
    })
    .expect("compile succeeds");
    let body = inherent_method_body(tokens, "process");

    let clear_thru = "self . thru . clear ()";
    let forward = "& mut self . thru";
    let pos_forward = body
        .find(forward)
        .expect("process() should forward events into `thru`");
    if let Some(pos_clear) = body.find(clear_thru) {
        assert!(
            pos_clear < pos_forward,
            "event output must be cleared BEFORE it is populated:\n{}",
            body
        );
    }
    let after_forward = &body[pos_forward..];
    assert!(
        !after_forward.contains(clear_thru),
        "event output must not be cleared after it is populated:\n{}",
        body
    );
    // Event inputs are still cleared after processing.
    let pos_clear_midi = body
        .rfind("self . midi . clear ()")
        .expect("event input should be cleared");
    assert!(
        pos_clear_midi > pos_forward,
        "event input clearing should happen after processing:\n{}",
        body
    );
}

// ---------------------------------------------------------------------------
// Bare event input -> event output passthrough (`midi -> thru`)
// ---------------------------------------------------------------------------

#[test]
fn bare_event_passthrough_emits_queue_copy() {
    let tokens = compile(quote! {
        name: EvThru;
        input event midi;
        output event thru;
        connections {
            midi -> thru;
        }
    })
    .expect("compile succeeds");
    let body = inherent_method_body(tokens, "process");
    assert!(
        body.contains("(& self . midi , & mut self . thru)"),
        "expected a queue copy from `midi` to `thru`; got:\n{}",
        body
    );
}

// ---------------------------------------------------------------------------
// Indexed endpoints on same-rate paths
// ---------------------------------------------------------------------------

#[test]
fn indexed_source_and_dest_use_single_element_access() {
    let tokens = compile_to_string(quote! {
        name: IdxCode;
        input stream s;
        output stream out;
        output stream solo;
        node voices = [Gain::new(0.5); 3];
        node fx = Gain::new(0.5);
        node lfo = Gain::new(0.5);
        connections {
            s -> voices.input;
            lfo.output -> voices[2].gain;
            voices[0].output -> fx.input;
            voices[1].output -> solo;
            fx.output -> out;
        }
    });
    // `voices[0].output -> fx.input` reads exactly one element (no fan-in sum).
    assert!(
        tokens.contains("self . voices [0usize] . output"),
        "expected single-element read of voices[0]; got:\n{}",
        tokens
    );
    assert!(
        !tokens.contains("self . fx . input = self . voices . iter ()"),
        "indexed source must not fan-in over the whole array:\n{}",
        tokens
    );
    // `lfo.output -> voices[2].gain` writes exactly one element (no broadcast).
    assert!(
        tokens.contains("& mut self . voices [2usize] . gain"),
        "expected single-element write to voices[2]; got:\n{}",
        tokens
    );
    // `voices[1].output -> solo` (graph output) also reads one element.
    assert!(
        tokens.contains("self . voices [1usize] . output"),
        "expected single-element read of voices[1] for graph output; got:\n{}",
        tokens
    );
    assert!(
        !tokens.contains("self . solo = self . voices . iter ()"),
        "indexed graph-output source must not fan-in over the whole array:\n{}",
        tokens
    );
}

// ---------------------------------------------------------------------------
// Post-inner taint propagation (multirate scheduling)
// ---------------------------------------------------------------------------

#[test]
fn compound_source_taints_post_inner_consumer() {
    // `d` consumes a Down edge (post-inner). `a.output + d.output -> mix.input`
    // must schedule `mix` post-inner even though the leftmost operand is the
    // untainted `a`.
    let tokens = compile(quote! {
        name: TaintCompound;
        input stream s;
        output stream out;
        node up = Gain::new(0.5) * 2;
        node a = Gain::new(0.5);
        node d = Gain::new(0.5);
        node mix = Gain::new(0.5);
        connections {
            s -> up.input;
            s -> a.input;
            up.output -> d.input;
            a.output + d.output -> mix.input;
            mix.output -> out;
        }
    })
    .expect("compile succeeds");
    let body = inherent_method_body(tokens, "process");
    let pos_d = body
        .find("self . d . process ()")
        .expect("d should be processed");
    let pos_mix = body
        .find("self . mix . process ()")
        .expect("mix should be processed");
    assert!(
        pos_mix > pos_d,
        "mix consumes tainted d and must run post-inner (after d):\n{}",
        body
    );
}

#[test]
fn same_rate_event_edge_propagates_post_inner_taint() {
    // `d` is post-inner; the same-rate event edge `d.midi_out -> sink.midi_in`
    // must pull `sink` post-inner too (its events would otherwise be a frame
    // stale).
    let tokens = compile(quote! {
        name: TaintEvent;
        input stream s;
        input event midi;
        output stream out;
        node up = Gain::new(0.5) * 2;
        node d = Gain::new(0.5);
        node sink = Gain::new(0.5);
        connections {
            s -> up.input;
            up.output -> d.input;
            midi -> sink.midi_in;
            d.midi_out -> sink.midi_in;
            sink.output -> out;
        }
    })
    .expect("compile succeeds");
    let body = inherent_method_body(tokens, "process");
    let pos_d = body
        .find("self . d . process ()")
        .expect("d should be processed");
    let pos_sink = body
        .find("self . sink . process ()")
        .expect("sink should be processed");
    assert!(
        pos_sink > pos_d,
        "sink consumes a same-rate event edge from tainted d and must run post-inner:\n{}",
        body
    );
}
