//! Token-stream snapshot tests. Guard against accidental regressions in
//! generated code during the emitter split (Tasks 13-15) and any future
//! emitter refactor.
//!
//! Snapshots live in `tests/snapshots/*.tokens` as the
//! `proc_macro2::TokenStream::to_string()` form. Updating snapshots after
//! an intentional change: set `OSCEN_UPDATE_SNAPSHOTS=1`.

use oscen_graph_compiler::compile;
use std::fs;
use std::path::PathBuf;

fn snapshot_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("snapshots");
    p
}

fn compare_snapshot(name: &str, actual: String) {
    let mut path = snapshot_dir();
    fs::create_dir_all(&path).unwrap();
    path.push(format!("{}.tokens", name));

    if std::env::var("OSCEN_UPDATE_SNAPSHOTS").is_ok() {
        fs::write(&path, &actual).expect("write snapshot");
        return;
    }

    if !path.exists() {
        fs::write(&path, &actual).expect("write initial snapshot");
        panic!(
            "snapshot {:?} did not exist; created. Re-run to confirm.",
            path
        );
    }

    let expected = fs::read_to_string(&path).expect("read snapshot");
    assert_eq!(
        actual.trim(),
        expected.trim(),
        "snapshot {:?} drifted. Run with OSCEN_UPDATE_SNAPSHOTS=1 to update.",
        path
    );
}

/// A minimal same-rate graph: one input stream, one processor, one output.
#[test]
fn snapshot_simple_same_rate_graph() {
    let input = quote::quote! {
        name: SimpleGraph;
        input stream s;
        output stream out;
        node filter = TptFilter::new(1000.0, 0.7);
        connections {
            s -> filter.input;
            filter.output -> out;
        }
    };
    let tokens = compile(input).expect("compile succeeds").to_string();
    compare_snapshot("simple_same_rate", tokens);
}

/// An oversampled graph: a processor running at 4× the graph rate feeds a
/// stream output through a cross-rate edge.
#[test]
fn snapshot_oversampled_graph() {
    let input = quote::quote! {
        name: OversampledGraph;
        input stream s;
        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.6) * 4;
        connections {
            s -> osc.frequency;
            [sinc] osc.output -> out;
        }
    };
    let tokens = compile(input).expect("compile succeeds").to_string();
    compare_snapshot("oversampled_graph", tokens);
}

/// A graph with value inputs (plain + ramped + full metadata) exercising the
/// generated parameter registry: `{Graph}Param` enum, `param_descriptors()`,
/// and the `set_param`/`set_param_immediate`/`get_param` dispatchers.
#[test]
fn snapshot_param_registry_graph() {
    let input = quote::quote! {
        name: ParamGraph;
        input value gain = 0.5;
        input cutoff: value = 1000.0 [20.0..20000.0, log, unit = "Hz", ramp: 64];
        input drive: value = 1.0 [0.0..10.0, center = 2.0, step = 0.1, group = "Tone"];
        output stream out;
        node osc = PolyBlepOscillator::saw(440.0, 0.6);
        node filter = TptFilter::new(1000.0, 0.7);
        connections {
            cutoff -> filter.cutoff;
            osc.output * gain * drive -> filter.input;
            filter.output -> out;
        }
    };
    let tokens = compile(input).expect("compile succeeds").to_string();
    compare_snapshot("param_registry", tokens);
}
