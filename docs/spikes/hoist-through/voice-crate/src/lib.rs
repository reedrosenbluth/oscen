// Simulates what #[derive(Node)] would emit for FMVoice:
// the struct itself plus a "manifest" macro_rules that carries its
// endpoint list and invokes a caller-supplied continuation with it.

pub struct FMVoice {
    pub frequency: f32,
    pub gate: f32,
    pub op3_ratio: f32,
    pub audio_out: f32,
}

impl FMVoice {
    pub fn new() -> Self {
        Self { frequency: 440.0, gate: 0.0, op3_ratio: 1.0, audio_out: 0.0 }
    }
}

// Manifest macro. #[macro_export] puts it at crate root; the doc(hidden)
// re-export next to the type lets `use voice_crate::*` or an explicit
// `use` bring it into scope alongside FMVoice.
#[macro_export]
#[doc(hidden)]
macro_rules! __oscen_endpoints_FMVoice {
    // callback!( <passthrough tokens> ; endpoints for <name>: ... )
    ($callback:ident ! ( $($passthrough:tt)* )) => {
        $callback! {
            $($passthrough)*
            inputs [ frequency: value = 440.0, gate: value = 0.0, op3_ratio: value = 1.0 ]
            outputs [ audio_out: stream ]
        }
    };
}

// Second node type, to test chaining two manifests (multi-wildcard graphs).
pub struct NoiseVoice { pub level: f32, pub noise_out: f32 }
impl NoiseVoice { pub fn new() -> Self { Self { level: 0.5, noise_out: 0.0 } } }

#[macro_export]
#[doc(hidden)]
macro_rules! __oscen_endpoints_NoiseVoice {
    ($callback:path => ( $($passthrough:tt)* )) => {
        $callback! {
            $($passthrough)*
            inputs [ level: value = 0.5 ]
            outputs [ noise_out: stream ]
        }
    };
}

// Also give FMVoice a path-based variant to prove `$callback:path` works
// (needed so the continuation can be a proc macro in another crate).
#[macro_export]
#[doc(hidden)]
macro_rules! __oscen_endpoints_FMVoice_path {
    ($callback:path => ( $($passthrough:tt)* )) => {
        $callback! {
            $($passthrough)*
            inputs [ frequency: value = 440.0, gate: value = 0.0, op3_ratio: value = 1.0 ]
            outputs [ audio_out: stream ]
        }
    };
}
