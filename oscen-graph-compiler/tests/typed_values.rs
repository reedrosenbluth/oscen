//! Stage-2 typed value endpoints: compiler-level tests.
//!
//! A graph value input/output declared with a non-f32 type
//! (`input mode: value: FilterMode;`) is TYPED: it becomes a real field of
//! that type with a typed setter, is connected per-frame by copy, is
//! excluded from the param registry and the nih-plug params struct, cannot
//! carry a param spec or fan in, and is latch-only across rate boundaries.
//! These tests assert on generated tokens and on lowering diagnostics; the
//! end-to-end runtime behaviour lives in
//! `oscen-lib/tests/typed_value_endpoints.rs`.

use oscen_graph_compiler::compile;
use quote::quote;

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

/// Compile a graph expected to fail and return the diagnostic messages.
fn compile_errors(tokens: proc_macro2::TokenStream) -> Vec<String> {
    match compile(tokens) {
        Ok(_) => panic!("compile unexpectedly succeeded"),
        Err(diags) => diags.items.iter().map(|d| d.message.to_string()).collect(),
    }
}

// ---------------------------------------------------------------------------
// Fields, init, setters
// ---------------------------------------------------------------------------

#[test]
fn typed_value_input_generates_typed_field_init_and_setter() {
    let tokens = compile_to_string(quote! {
        name: TypedIo;
        input mode: value: FilterMode;
        input tuned: value: FilterMode = FilterMode::Highpass;
        output active: value: FilterMode;
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode -> f.mode;
            f.active_mode -> active;
            f.out -> out;
        }
    });

    // Fields carry the declared type (inputs and outputs alike).
    assert!(
        tokens.contains("pub mode : FilterMode"),
        "typed input field"
    );
    assert!(
        tokens.contains("pub active : FilterMode"),
        "typed output field"
    );

    // Init: `= expr` used as-is; no default falls back to T::default();
    // typed outputs start at T::default().
    assert!(
        tokens.contains("let tuned = FilterMode :: Highpass ;"),
        "explicit typed default used as-is"
    );
    assert!(
        tokens.contains("let mode = < FilterMode as :: core :: default :: Default > :: default ()"),
        "typed input without default starts at T::default()"
    );
    assert!(
        tokens
            .contains("let active = < FilterMode as :: core :: default :: Default > :: default ()"),
        "typed output starts at T::default()"
    );

    // Setter takes the declared type, not f32.
    assert!(
        tokens.contains("pub fn set_mode (& mut self , value : FilterMode)"),
        "typed setter signature"
    );

    // No param registry: every value input is typed, so no Param enum.
    assert!(
        !tokens.contains("enum TypedIoParam"),
        "typed-only graph must not emit a param enum"
    );
}

#[test]
fn f32_annotation_is_a_plain_param_not_typed() {
    // `: f32` normalizes to the plain param path: f32 field, f32 setter,
    // registry entry.
    let tokens = compile_to_string(quote! {
        name: PlainF32;
        input gain: value: f32 = 0.5;
        output stream out;
        node g = Gain::new(0.5);
        connections {
            gain -> g.gain;
            g.output -> out;
        }
    });
    assert!(tokens.contains("pub gain : f32"), "f32 field");
    assert!(
        tokens.contains("pub fn set_gain (& mut self , value : f32)"),
        "f32 setter"
    );
    assert!(
        tokens.contains("enum PlainF32Param"),
        "f32-annotated input stays in the registry"
    );
}

// ---------------------------------------------------------------------------
// Registry partition
// ---------------------------------------------------------------------------

#[test]
fn registry_covers_only_f32_params() {
    // Typed input declared FIRST so any indexing bug between the registry
    // and the nih wrapper would misalign.
    let tokens = compile_to_string(quote! {
        name: Mixed;
        input mode: value: FilterMode;
        input value gain = 0.5;
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode -> f.mode;
            gain -> f.gain;
            f.out -> out;
        }
    });

    // Enum has exactly the f32 param.
    assert!(
        tokens.contains("pub enum MixedParam { Gain , }"),
        "enum covers only f32 params; got:\n{}",
        &tokens[tokens.find("enum MixedParam").unwrap_or(0)..][..200.min(tokens.len())]
    );
    assert!(
        !tokens.contains("MixedParam :: Mode"),
        "typed input must not get an enum variant"
    );
    // Dispatchers only route the f32 param.
    assert!(tokens.contains("MixedParam :: Gain => self . set_gain (value)"));
    // The typed setter still exists.
    assert!(tokens.contains("pub fn set_mode (& mut self , value : FilterMode)"));
    // Descriptor table has one entry.
    assert!(tokens.contains("[:: oscen :: graph :: ParamDescriptor ; 1usize]"));
}

#[test]
fn nih_params_struct_omits_typed_inputs_and_keeps_descriptor_alignment() {
    let tokens = compile_to_string(quote! {
        name: NihMixed;
        nih_params;
        input mode: value: FilterMode;
        input value gain = 0.5;
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode -> f.mode;
            gain -> f.gain;
            f.out -> out;
        }
    });

    // Params struct has only the f32 param field.
    assert!(
        tokens.contains("pub gain : :: nih_plug :: prelude :: FloatParam"),
        "f32 param present in nih struct"
    );
    assert!(
        !tokens.contains("pub mode : :: nih_plug :: prelude :: FloatParam"),
        "typed input must not become a FloatParam"
    );
    // Positional default lookup uses the FILTERED table index: `gain` is the
    // second declared value input but the first (index 0) param.
    assert!(
        tokens.contains("NihMixed :: param_descriptors () [0usize] . default"),
        "nih default lookup must index the filtered descriptor table"
    );
    // sync_to only syncs the f32 param.
    assert!(tokens.contains("graph . gain = self . gain . value ()"));
    assert!(!tokens.contains("graph . mode = self . mode . value ()"));
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[test]
fn graph_manifest_carries_typed_value_ty_tokens() {
    let tokens = compile_to_string(quote! {
        name: ManifestTyped;
        input mode: value: FilterMode;
        input value gain = 0.5;
        output active: value: FilterMode;
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode -> f.mode;
            gain -> f.gain;
            f.active_mode -> active;
            f.out -> out;
        }
    });
    assert!(
        tokens.contains("mode : value (ty = FilterMode)"),
        "typed input manifest entry carries ty tokens"
    );
    assert!(
        tokens.contains("active : value (ty = FilterMode)"),
        "typed output manifest entry carries ty tokens"
    );
    // The plain f32 param stays annotation-free.
    assert!(tokens.contains("gain : value]") || tokens.contains("gain : value ,"));
}

// ---------------------------------------------------------------------------
// Cross-rate: latch at the outer-block boundary
// ---------------------------------------------------------------------------

#[test]
fn typed_cross_rate_edges_latch_without_kernel_state() {
    let tokens = compile_to_string(quote! {
        name: TypedXRate;
        input mode: value: FilterMode;
        output active: value: FilterMode;
        output stream out;
        node f = ModeFilter::new() * 2;
        connections {
            mode -> f.mode;
            f.active_mode -> active;
            [sinc] f.out -> out;
        }
    });

    // No f32 latch kernels for the typed edges (the stream edge still gets
    // its resampler).
    assert!(
        !tokens.contains("LatchUp") && !tokens.contains("LatchDown"),
        "typed value edges must not allocate f32 latch kernel state"
    );
    // Both directions emit a plain typed copy.
    assert!(
        tokens.contains(":: connect (& self . mode , & mut self . f . mode ,)"),
        "up-direction latch copies the typed input once per outer tick"
    );
    assert!(
        tokens.contains(":: connect (& self . f . active_mode , & mut self . active ,)"),
        "down-direction latch copies the typed output at the block boundary"
    );
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[test]
fn param_spec_on_typed_value_input_errors() {
    let msgs = compile_errors(quote! {
        name: SpecTyped;
        input mode: value: FilterMode = FilterMode::Lowpass [ramp: 64];
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode -> f.mode;
            f.out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("typed value input `mode` cannot carry a param spec")),
        "expected spec-on-typed error; got: {msgs:?}"
    );
}

#[test]
fn fan_in_into_typed_value_output_errors() {
    let msgs = compile_errors(quote! {
        name: FaninTyped;
        output active: value: FilterMode;
        output stream out;
        node a = ModeFilter::new();
        node b = ModeFilter::new();
        connections {
            a.active_mode -> active;
            b.active_mode -> active;
            a.out -> out;
        }
    });
    assert!(
        msgs.iter().any(
            |m| m.contains("typed value endpoint `active` has 2 sources")
                && m.contains("values don't sum")
        ),
        "expected typed fan-in error; got: {msgs:?}"
    );
}

#[test]
fn typed_source_fan_in_into_node_endpoint_errors() {
    // Two typed graph inputs into the same node endpoint: fan-in is caught
    // even though the dest is a node field the compiler can't type.
    let msgs = compile_errors(quote! {
        name: FaninNodeDest;
        input mode_a: value: FilterMode;
        input mode_b: value: FilterMode;
        output stream out;
        node f = ModeFilter::new();
        connections {
            mode_a -> f.mode;
            mode_b -> f.mode;
            f.out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("typed value endpoint `f.mode` has 2 sources")),
        "expected typed fan-in error; got: {msgs:?}"
    );
}

#[test]
fn array_fan_in_into_typed_value_output_errors() {
    let msgs = compile_errors(quote! {
        name: ArrayFaninTyped;
        output active: value: FilterMode;
        output stream out;
        node voices = [ModeFilter::new(); 4];
        connections {
            voices.active_mode -> active;
            voices[0].out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("typed values cannot fan in from a node array")),
        "expected array fan-in error; got: {msgs:?}"
    );
}

#[test]
fn non_latch_policy_on_typed_cross_rate_edge_errors() {
    let msgs = compile_errors(quote! {
        name: PolicyTyped;
        input mode: value: FilterMode;
        output stream out;
        node f = ModeFilter::new() * 2;
        connections {
            [linear] mode -> f.mode;
            [sinc] f.out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("typed value connections are latch-only across rate boundaries")),
        "expected latch-only error; got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// f32 value fan-in rejection (adversarial-review fix A4)
// ---------------------------------------------------------------------------

#[test]
fn f32_value_fan_in_errors() {
    // COOKBOOK: only streams sum. Two sources into one f32 value endpoint
    // used to compile silently with last-write-wins.
    let msgs = compile_errors(quote! {
        name: G;
        input value a;
        input value b;
        output value out;
        connections {
            a -> out;
            b -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("values cannot fan in")),
        "expected value fan-in error; got {msgs:?}"
    );
}

#[test]
fn f32_value_fan_in_into_node_endpoint_errors() {
    // Fan-in into a node's value endpoint whose kind is known through
    // inference from the graph value input.
    let msgs = compile_errors(quote! {
        name: G;
        input value cutoff;
        input stream s;
        output stream out;
        nodes {
            lfo = PolyBlepOscillator::sine(2.0, 0.5);
            flt = TptFilter::new(1000.0, 0.7);
        }
        connections {
            s -> flt.input;
            cutoff -> flt.cutoff;
            lfo.output -> flt.cutoff;
            flt.output -> out;
        }
    });
    assert!(
        msgs.iter().any(|m| m.contains("cannot fan in")),
        "expected value fan-in error; got {msgs:?}"
    );
}

#[test]
fn f32_array_value_fan_in_errors() {
    // An array of plain f32 value outputs feeding one value dest sums the
    // elements — held to the same no-fan-in rule as typed payloads.
    let msgs = compile_errors(quote! {
        name: ArrayFaninF32;
        output value m;
        output stream out;
        node voices = [ModeFilter::new(); 4];
        connections {
            voices.gain_out -> m;
            voices[0].out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("values cannot fan in from a node array")),
        "expected array value fan-in error; got {msgs:?}"
    );
}

#[test]
fn broadcast_and_indexed_value_dest_errors() {
    // `all -> voices.gain` drives every element, so `one -> voices[0].gain`
    // gives element 0 two drivers with connection order deciding.
    let msgs = compile_errors(quote! {
        name: BroadcastIndexed;
        input value all;
        input value one;
        output stream out;
        node voices = [ModeFilter::new(); 4];
        connections {
            all -> voices.gain;
            one -> voices[0].gain;
            voices[0].out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("driven both directly and by a broadcast connection")),
        "expected broadcast/indexed conflict error; got {msgs:?}"
    );
}

#[test]
fn broadcast_and_indexed_typed_dest_errors() {
    // Same conflict through typed graph inputs: caught via the typed edges
    // even when the node endpoint's kind is unknown to the IR.
    let msgs = compile_errors(quote! {
        name: BroadcastIndexedTyped;
        input all: value: FilterMode;
        input one: value: FilterMode;
        output stream out;
        node voices = [ModeFilter::new(); 4];
        connections {
            all -> voices.mode;
            one -> voices[0].mode;
            voices[0].out -> out;
        }
    });
    assert!(
        msgs.iter()
            .any(|m| m.contains("driven both directly and by a broadcast connection")),
        "expected broadcast/indexed conflict error; got {msgs:?}"
    );
}

#[test]
fn broadcast_and_indexed_stream_dest_still_compiles() {
    // The conflict rule is value-only: overlapping stream drivers keep the
    // summing semantics streams already have.
    let tokens = compile_to_string(quote! {
        name: BroadcastIndexedStream;
        input stream a;
        input stream b;
        output stream out;
        node voices = [ModeFilter::new(); 4];
        connections {
            a -> voices.input;
            b -> voices[0].input;
            voices[0].out -> out;
        }
    });
    assert!(!tokens.is_empty());
}

#[test]
fn stream_fan_in_still_sums() {
    // The fan-in rejection is value-only: stream fan-in keeps summing.
    let tokens = compile_to_string(quote! {
        name: G;
        input stream a;
        input stream b;
        output stream out;
        connections {
            a -> out;
            b -> out;
        }
    });
    assert!(!tokens.is_empty());
}
