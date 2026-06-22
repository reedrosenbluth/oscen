#![feature(inherent_associated_types)]

// An `asset` endpoint may only be fed by an `external` declaration. Wiring a
// per-sample `stream` source into it (here `dry -> reverb.ir`, alongside the
// legitimate `ir -> reverb.ir` binding) must be rejected with a clear,
// asset-specific diagnostic — not a leaked internal type error.

use oscen::graph;

graph! {
    name: AssetEndpointMismatch;

    input stream dry;
    output stream wet;

    external ir: AudioAsset;

    nodes {
        reverb = Convolver::new();
    }

    connections {
        dry -> reverb.input;
        reverb.output -> wet;
        ir -> reverb.ir;
        dry -> reverb.ir;  // stream into an asset endpoint: should not compile.
    }
}

fn main() {}
