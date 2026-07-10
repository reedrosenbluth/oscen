//! End-to-end coverage for the adversarial-review fixes on this branch:
//!
//! - B3: wildcard-hoisting a node with a raw-ident endpoint field
//!   (`r#loop`) compiles and works (used to panic the proc macro);
//! - B4: `param_descriptors()` succeeds from a small-stack thread on a
//!   graph with hoist-inherited defaults (the probe graph builds on an
//!   internal big-stack thread);
//! - B5: param metadata (range/log/unit/center/step/group) declared on a
//!   nested `graph!` child's input survives a wildcard hoist into the
//!   parent's registry;
//! - B6: a `pub(crate)` endpoint field wildcard-hoists from a same-crate
//!   graph (used to be silently skipped as private);
//! - B7: under `use oscen::prelude::*`, a user-defined node wildcard-hoists
//!   when referenced by qualified path (the glob makes rustc reject bare
//!   resolution of the derive-emitted manifest alias — macro-expanded names
//!   cannot shadow glob imports), alongside a bare prelude node.
#![feature(inherent_associated_types)]

use oscen::{graph, Node, PolyBlepOscillator, SignalProcessor};

const RATE: f32 = 48_000.0;

// ---------------------------------------------------------------------------
// B3: raw-ident endpoint field
// ---------------------------------------------------------------------------

#[derive(Debug, Node)]
pub struct Looper {
    /// A keyword-named endpoint: natural for loop/sync-style controls.
    #[input(value)]
    pub r#loop: f32,
    #[output(stream)]
    pub out: f32,
}

impl Looper {
    pub fn new() -> Self {
        Self {
            r#loop: 0.25,
            out: 0.0,
        }
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for Looper {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.r#loop;
    }
}

graph! {
    name: RawIdentHoist;

    output stream out;

    nodes {
        lp = Looper::new();
    }

    input lp.*;

    connections {
        lp.out -> out;
    }
}

#[test]
fn raw_ident_endpoint_wildcard_hoists() {
    let mut g = RawIdentHoist::new();
    g.init(RATE);
    // The hoisted input keeps the raw-ident field name and inherits the
    // child's constructor default.
    assert_eq!(g.r#loop, 0.25);
    // Derived names use the bare name: setter `set_loop`, variant `Loop`.
    g.set_loop(0.75);
    g.process();
    assert_eq!(g.get_stream_output(0), Some(0.75));
    let p = RawIdentHoistParam::from_name("loop").expect("param registered under bare name");
    assert_eq!(p, RawIdentHoistParam::Loop);
}

// ---------------------------------------------------------------------------
// B4: param_descriptors() from a small-stack thread
// ---------------------------------------------------------------------------

/// A deliberately large voice (256 KiB of state) so the probe graph below
/// dwarfs the test thread's stack.
#[derive(Debug, Node)]
pub struct BigVoice {
    #[input(value)]
    pub pitch: f32,
    #[output(stream)]
    pub out: f32,
    buf: [f32; 65536],
}

impl BigVoice {
    pub fn new() -> Self {
        Self {
            pitch: 220.0,
            out: 0.0,
            buf: [0.0; 65536],
        }
    }
}

impl Default for BigVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for BigVoice {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.buf[0] + self.pitch;
    }
}

graph! {
    name: SmallStackProbe;

    output stream out;

    nodes {
        voices = [BigVoice::new(); 4];
    }

    // Wildcard hoists inherit defaults from the child constructor, which
    // forces the descriptor table to build a probe graph instance — a
    // ~1 MiB struct here, far over the test thread's 128 KiB stack.
    input voices.*;

    connections {
        voices.out -> out;
    }
}

#[test]
fn param_descriptors_survive_small_stack_thread() {
    // nih-plug hosts call Params::default() (and thus param_descriptors)
    // from arbitrary threads; 128 KiB is far below the graph's size class.
    let handle = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| SmallStackProbe::param_descriptors().len())
        .expect("spawn small-stack thread");
    let len = handle.join().expect("small-stack thread must not overflow");
    assert!(len > 0);
}

// ---------------------------------------------------------------------------
// B5: nested-graph metadata survives a wildcard hoist
// ---------------------------------------------------------------------------

graph! {
    name: InnerMeta;

    input cutoff: value = 1000.0 [20.0..20000.0, log, unit = "Hz", group = "Filter"];
    output stream wet;

    nodes {
        osc = PolyBlepOscillator::saw(440.0, 0.5);
    }

    connections {
        cutoff -> osc.frequency;
        osc.output -> wet;
    }
}

graph! {
    name: OuterMeta;

    output stream out;

    nodes {
        v = InnerMeta::new();
    }

    input v.*;

    connections {
        v.wet -> out;
    }
}

#[test]
fn nested_graph_hoist_preserves_param_metadata() {
    let p = OuterMetaParam::from_name("cutoff").expect("hoisted param exists");
    let d = p.descriptor();
    assert_eq!(
        d.range,
        Some((20.0, 20000.0)),
        "range must survive the hoist"
    );
    assert!(d.logarithmic, "log curve must survive the hoist");
    assert_eq!(d.unit, Some("Hz"), "unit must survive the hoist");
    assert_eq!(d.group, Some("Filter"), "group must survive the hoist");
    assert_eq!(d.default, 1000.0);
}

// ---------------------------------------------------------------------------
// B6: pub(crate) endpoint hoists in-crate
// ---------------------------------------------------------------------------

#[derive(Debug, Node)]
pub struct CrateGain {
    #[input(value)]
    pub(crate) amount: f32,
    #[output(stream)]
    pub out: f32,
}

impl CrateGain {
    pub fn new() -> Self {
        Self {
            amount: 0.5,
            out: 0.0,
        }
    }
}

impl Default for CrateGain {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor for CrateGain {
    #[inline(always)]
    fn process(&mut self) {
        self.out = self.amount;
    }
}

graph! {
    name: CrateVisHoist;

    output stream out;

    nodes {
        g = CrateGain::new();
    }

    input g.*;

    connections {
        g.out -> out;
    }
}

#[test]
fn pub_crate_endpoint_wildcard_hoists_in_crate() {
    let mut g = CrateVisHoist::new();
    g.init(RATE);
    assert_eq!(
        g.amount, 0.5,
        "pub(crate) endpoint must hoist, not be skipped"
    );
    g.set_amount(0.9);
    g.process();
    assert_eq!(g.get_stream_output(0), Some(0.9));
}

// ---------------------------------------------------------------------------
// B7: user node shadowing a prelude node name, under the prelude glob
// ---------------------------------------------------------------------------

mod prelude_shadow {
    use oscen::prelude::*;

    pub mod dsp {
        use oscen::{Node, SignalProcessor};

        /// Deliberately shadows the prelude's `Gain` node type name.
        #[derive(Debug, Node)]
        pub struct Gain {
            #[input(value)]
            pub level: f32,
            #[output(stream)]
            pub out: f32,
        }

        impl Gain {
            pub fn new() -> Self {
                Self {
                    level: 0.3,
                    out: 0.0,
                }
            }
        }

        impl Default for Gain {
            fn default() -> Self {
                Self::new()
            }
        }

        impl SignalProcessor for Gain {
            #[inline(always)]
            fn process(&mut self) {
                self.out = self.level;
            }
        }
    }

    graph! {
        name: ShadowedManifest;

        output stream out;

        nodes {
            // Qualified path: the manifest invocation follows it
            // (`dsp::__oscen_endpoints_Gain!`), sidestepping the
            // glob-vs-expanded ambiguity a bare `Gain` would hit under
            // `use oscen::prelude::*`.
            user_g = dsp::Gain::new();
            // A prelude node hoisted bare, in the same graph, still
            // resolves through the prelude alias.
            osc = PolyBlepOscillator::saw(440.0, 0.5);
        }

        input user_g.*;
        input osc.*;

        connections {
            user_g.out + osc.output -> out;
        }
    }

    #[test]
    fn shadowed_node_hoists_via_qualified_path() {
        let mut g = ShadowedManifest::new();
        g.init(super::RATE);
        assert_eq!(g.level, 0.3, "user Gain's endpoint hoisted");
        // The prelude oscillator's endpoints hoisted alongside.
        g.set_level(1.0);
        g.process();
    }
}
