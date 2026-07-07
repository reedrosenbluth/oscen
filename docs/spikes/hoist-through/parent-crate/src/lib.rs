// Simulates the parent graph crate. The real graph! proc macro, upon
// seeing `input voices.*;` with `voices = poly::<8>(FMVoice::new())`,
// cannot know FMVoice's endpoints. Instead it expands to an invocation
// of the manifest macro with a continuation that receives the endpoint
// list and performs final codegen. Here the "continuation" is a
// macro_rules stand-in that generates a struct with one field per
// hoisted input, to prove tokens flow end to end across crates.

use voice_crate::FMVoice;

macro_rules! finish_graph {
    (
        graph_name $g:ident
        inputs [ $($in_name:ident : $in_kind:ident = $default:expr),* $(,)? ]
        outputs [ $($out_name:ident : $out_kind:ident),* $(,)? ]
    ) => {
        pub struct $g {
            $(pub $in_name: f32,)*
            $(pub $out_name: f32,)*
            pub voices: [FMVoice; 8],
        }
        impl $g {
            pub fn new() -> Self {
                Self {
                    $($in_name: $default,)*
                    $($out_name: 0.0,)*
                    voices: core::array::from_fn(|_| FMVoice::new()),
                }
            }
        }
    };
}

// What graph! would emit for `input voices.*` — call the manifest with
// our continuation and passthrough state:
voice_crate::__oscen_endpoints_FMVoice!(finish_graph!(graph_name SynthGraph));

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hoisted_fields_exist_with_defaults() {
        let g = SynthGraph::new();
        assert_eq!(g.frequency, 440.0);
        assert_eq!(g.op3_ratio, 1.0);
        assert_eq!(g.audio_out, 0.0);
        assert_eq!(g.voices.len(), 8);
    }
}

// --- chaining test: two wildcard nodes, CPS-style ---
// Step 1 collects FMVoice's endpoints, step 2 collects NoiseVoice's,
// final step generates code from both. Each step is what graph! would
// emit as intermediate macros.

macro_rules! chain_step2 {
    (
        state { $($state:tt)* }
        inputs [ $($i:tt)* ] outputs [ $($o:tt)* ]
    ) => {
        // captured fm endpoints in $state; now fetch noise endpoints
        voice_crate::__oscen_endpoints_NoiseVoice!(
            chain_final => ( fm { inputs [ $($i)* ] outputs [ $($o)* ] } $($state)* )
        );
    };
}

macro_rules! chain_final {
    (
        fm { inputs [ $($fi_name:ident : $fi_k:ident = $fi_d:expr),* $(,)? ]
             outputs [ $($fo:ident : $fo_k:ident),* $(,)? ] }
        graph_name $g:ident
        inputs [ $($ni_name:ident : $ni_k:ident = $ni_d:expr),* $(,)? ]
        outputs [ $($no:ident : $no_k:ident),* $(,)? ]
    ) => {
        pub struct $g {
            $(pub $fi_name: f32,)*
            $(pub $ni_name: f32,)*
        }
        impl $g {
            pub fn new() -> Self {
                Self { $($fi_name: $fi_d,)* $($ni_name: $ni_d,)* }
            }
        }
    };
}

voice_crate::__oscen_endpoints_FMVoice_path!(chain_step2 => ( state { graph_name TwoKindGraph } ));

#[cfg(test)]
mod chain_tests {
    use super::*;
    #[test]
    fn two_manifests_chain() {
        let g = TwoKindGraph::new();
        assert_eq!(g.frequency, 440.0);
        assert_eq!(g.level, 0.5);
    }
}

// --- proc-macro continuation test ---
voice_crate::__oscen_endpoints_FMVoice_path!(pm::finish => ( graph_name PmGraph ));

#[cfg(test)]
mod pm_tests {
    #[test]
    fn proc_macro_received_manifest_tokens() {
        assert!(super::TOKENS_SEEN == 6);
    }
}
