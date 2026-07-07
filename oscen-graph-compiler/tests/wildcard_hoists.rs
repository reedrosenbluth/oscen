//! Wildcard hoist (`input node.*;`) compiler tests: parsing, the two-stage
//! manifest expansion (scan → chain → resume), expansion skip rules, and
//! collision diagnostics. Cross-crate mechanics are exercised in
//! `oscen-lib/tests/wildcard_hoists.rs`; here we drive the compiler API
//! directly with hand-built manifest payloads.

use oscen_graph_compiler::manifest::{expand_graph_entry, resume, scan_wildcards};
use oscen_graph_compiler::{compile, Diagnostics};
use quote::quote;

fn error_messages(diags: &Diagnostics) -> Vec<String> {
    diags
        .items
        .iter()
        .map(|d| d.message.to_string())
        .collect()
}

fn resume_path() -> syn::Path {
    syn::parse_quote!(::oscen::__oscen_graph_resume)
}

// ---------------------------------------------------------------------------
// scan_wildcards
// ---------------------------------------------------------------------------

#[test]
fn scan_finds_wildcards_in_declaration_order() {
    let input = quote! {
        name: Wild;
        output stream out;
        nodes {
            a = FMVoice::new();
            b = some::path::NoiseVoice::new();
        }
        input b.*;
        input a.*;
        connections { a.output -> out; }
    };
    let reqs = scan_wildcards(input).expect("scan succeeds");
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].node.to_string(), "b");
    assert_eq!(reqs[1].node.to_string(), "a");
}

#[test]
fn scan_maps_type_path_to_manifest_path() {
    let input = quote! {
        name: Wild;
        output stream out;
        nodes {
            a = FMVoice::new();
            b = some::path::NoiseVoice::new();
        }
        input a.*;
        input b.*;
        connections { a.output -> out; }
    };
    let reqs = scan_wildcards(input).expect("scan succeeds");
    let paths: Vec<String> = reqs
        .iter()
        .map(|r| {
            let p = &r.manifest_path;
            quote!(#p).to_string().replace(' ', "")
        })
        .collect();
    assert_eq!(
        paths,
        vec![
            "__oscen_endpoints_FMVoice".to_string(),
            "some::path::__oscen_endpoints_NoiseVoice".to_string(),
        ]
    );
}

#[test]
fn scan_returns_empty_without_wildcards() {
    let input = quote! {
        name: Plain;
        output stream out;
        nodes { osc = Osc::new(); }
        input osc.frequency;
        connections { osc.output -> out; }
    };
    assert!(scan_wildcards(input).expect("scan succeeds").is_empty());
}

#[test]
fn scan_rejects_unknown_node_and_unresolvable_type() {
    let input = quote! {
        name: Bad;
        output stream out;
        nodes {
            a = make_voice();
        }
        input a.*;
        input ghost.*;
        connections { a.output -> out; }
    };
    let diags = match scan_wildcards(input) {
        Err(d) => d,
        Ok(_) => panic!("expected diagnostics"),
    };
    let msgs = error_messages(&diags);
    assert_eq!(msgs.len(), 2, "got: {msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("cannot determine the type")));
    assert!(msgs.iter().any(|m| m.contains("unknown node `ghost`")));
}

// ---------------------------------------------------------------------------
// Two-stage expansion: entry emits manifest invocation; resume chains and
// finishes.
// ---------------------------------------------------------------------------

#[test]
fn entry_without_wildcards_compiles_directly() {
    let input = quote! {
        name: Plain;
        input stream s;
        output stream out;
        nodes { filter = TptFilter::new(1000.0, 0.7); }
        connections {
            s -> filter.input;
            filter.output -> out;
        }
    };
    let direct = compile(input.clone()).expect("compile").to_string();
    let via_entry = expand_graph_entry(input, &resume_path())
        .expect("entry")
        .to_string();
    assert_eq!(direct, via_entry, "zero behavior change without wildcards");
}

#[test]
fn entry_with_wildcard_emits_manifest_invocation() {
    let input = quote! {
        name: Wild;
        output stream out;
        nodes { voices = [FMVoice::new(); 4]; }
        input voices.*;
        connections { voices.audio_out -> out; }
    };
    let tokens = expand_graph_entry(input, &resume_path())
        .expect("entry")
        .to_string();
    assert!(
        tokens.starts_with("__oscen_endpoints_FMVoice !"),
        "expected manifest invocation, got: {}",
        &tokens[..tokens.len().min(120)]
    );
    assert!(tokens.contains(":: oscen :: __oscen_graph_resume =>"));
    assert!(tokens.contains("current voices"));
    assert!(tokens.contains("graph {"));
}

/// Simulate the full chain for two wildcards: entry emits the first
/// manifest invocation; we play the manifest macro's role by appending
/// each node's endpoint payload and calling resume.
#[test]
fn resume_chains_manifests_then_compiles() {
    let graph_body = quote! {
        name: TwoWild;
        output stream out;
        nodes {
            fm = FMVoice::new();
            noise = NoiseVoice::new();
        }
        input fm.*;
        input noise.*;
        connections {
            fm.audio_out -> out;
            noise.noise_out -> out;
        }
    };

    let entry = expand_graph_entry(graph_body, &resume_path()).expect("entry");
    let entry_str = entry.to_string();
    assert!(entry_str.starts_with("__oscen_endpoints_FMVoice !"));
    assert!(entry_str.contains("pending [noise = __oscen_endpoints_NoiseVoice]"));

    // Extract the passthrough state from `manifest!(cb => ( STATE ));`.
    let state1 = extract_passthrough_state(&entry);

    // Play FMVoice's manifest: append its endpoint payload.
    let resume1_input = quote! {
        #state1
        node_type FMVoice
        inputs [ frequency: value, gate: value, op3_ratio: value ]
        outputs [ audio_out: stream ]
    };
    let step2 = resume(resume1_input, &resume_path()).expect("resume 1");
    let step2_str = step2.to_string();
    assert!(
        step2_str.starts_with("__oscen_endpoints_NoiseVoice !"),
        "second manifest chained: {}",
        &step2_str[..step2_str.len().min(120)]
    );
    assert!(step2_str.contains("resolved { fm {"));

    // Play NoiseVoice's manifest.
    let state2 = extract_passthrough_state(&step2);
    let resume2_input = quote! {
        #state2
        node_type NoiseVoice
        inputs [ level: value ]
        outputs [ noise_out: stream ]
    };
    let final_tokens = resume(resume2_input, &resume_path())
        .expect("final compile")
        .to_string();

    // All five child inputs hoisted, in manifest declaration order.
    assert!(final_tokens.contains("pub struct TwoWild"));
    for field in ["frequency", "gate", "op3_ratio", "level"] {
        assert!(
            final_tokens.contains(&format!("pub {field} : f32")),
            "hoisted field `{field}` missing"
        );
    }
    // Declaration order: registry enum order follows manifest order.
    let freq_pos = final_tokens.find("Frequency").expect("Frequency variant");
    let gate_pos = final_tokens.find("Gate").expect("Gate variant");
    let ratio_pos = final_tokens.find("Op3Ratio").expect("Op3Ratio variant");
    let level_pos = final_tokens.find("Level").expect("Level variant");
    assert!(freq_pos < gate_pos && gate_pos < ratio_pos && ratio_pos < level_pos);
}

/// Pull `STATE` out of `path ! (callback => ( STATE )) ;`.
fn extract_passthrough_state(tokens: &proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    // The invocation's only parenthesized group at top level is the
    // argument list `(callback => ( STATE ))`.
    let arg_group = tokens
        .clone()
        .into_iter()
        .find_map(|tt| match tt {
            proc_macro2::TokenTree::Group(g)
                if g.delimiter() == proc_macro2::Delimiter::Parenthesis =>
            {
                Some(g.stream())
            }
            _ => None,
        })
        .expect("argument group");
    // Inside: `::oscen::__oscen_graph_resume => ( STATE )`.
    arg_group
        .into_iter()
        .find_map(|tt| match tt {
            proc_macro2::TokenTree::Group(g)
                if g.delimiter() == proc_macro2::Delimiter::Parenthesis =>
            {
                Some(g.stream())
            }
            _ => None,
        })
        .expect("state group")
}

// ---------------------------------------------------------------------------
// Expansion semantics via compile_with_manifests
// ---------------------------------------------------------------------------

fn manifests_for(
    entries: &[(&str, proc_macro2::TokenStream)],
) -> std::collections::HashMap<String, oscen_graph_compiler::manifest::NodeManifest> {
    entries
        .iter()
        .map(|(name, tokens)| {
            (
                name.to_string(),
                syn::parse2(tokens.clone()).expect("manifest parses"),
            )
        })
        .collect()
}

#[test]
fn expansion_skips_connected_and_explicitly_hoisted_endpoints() {
    let input = quote! {
        name: SkipRules;
        input value drive = 1.0;
        output stream out;
        nodes {
            lfo = Osc::new();
            voice = FMVoice::new();
        }
        input voice.gate trig;
        input voice.*;
        connections {
            lfo.output -> voice.frequency;
            drive -> voice.op3_ratio;
            voice.audio_out -> out;
        }
    };
    let manifests = manifests_for(&[(
        "voice",
        quote! {
            node_type FMVoice
            inputs [ frequency: value, gate: value, op3_ratio: value, brightness: value ]
            outputs [ audio_out: stream ]
        },
    )]);
    let tokens = oscen_graph_compiler::compile_with_manifests(input, &manifests)
        .expect("compiles")
        .to_string();

    // `frequency` (lfo-driven) and `op3_ratio` (drive-driven) are
    // connection destinations: not hoisted.
    assert!(!tokens.contains("pub frequency"));
    assert!(!tokens.contains("pub op3_ratio"));
    // `gate` was hoisted explicitly (renamed to trig): the wildcard skips
    // it, the explicit hoist stands.
    assert!(tokens.contains("pub trig : f32"));
    assert!(!tokens.contains("pub gate"));
    // `brightness` is unclaimed: hoisted by the wildcard.
    assert!(tokens.contains("pub brightness : f32"));
}

#[test]
fn expansion_collision_with_declared_input_is_error() {
    let input = quote! {
        name: Collide;
        input value frequency = 100.0;
        output stream out;
        nodes { voice = FMVoice::new(); }
        input voice.*;
        connections { voice.audio_out -> out; }
    };
    let manifests = manifests_for(&[(
        "voice",
        quote! {
            node_type FMVoice
            inputs [ frequency: value, gate: value ]
            outputs [ audio_out: stream ]
        },
    )]);
    let diags = oscen_graph_compiler::compile_with_manifests(input, &manifests)
        .expect_err("collision must error");
    let msgs = error_messages(&diags);
    assert!(
        msgs.iter()
            .any(|m| m.contains("collides") && m.contains("frequency")),
        "got: {msgs:?}"
    );
}

#[test]
fn compile_without_manifests_reports_unresolved_wildcard() {
    let input = quote! {
        name: NoManifest;
        output stream out;
        nodes { voice = FMVoice::new(); }
        input voice.*;
        connections { voice.audio_out -> out; }
    };
    let diags = compile(input).expect_err("must error");
    let msgs = error_messages(&diags);
    assert!(
        msgs.iter().any(|m| m.contains("no endpoint manifest resolved")),
        "got: {msgs:?}"
    );
}
