// An `external` binds only to the node's asset input. Binding it to a signal
// endpoint, or to an endpoint that does not exist, fails at the endpoint
// rather than silently installing the node's asset consumer.

use oscen::graph;
use oscen::prelude::*;

graph! {
    name: AssetSignalEndpoint;

    output stream wet;

    external ir: AudioAsset;

    nodes {
        reverb = Convolver::new();
    }

    connections {
        reverb.output -> wet;
        ir -> reverb.input;
    }
}

graph! {
    name: AssetMissingEndpoint;

    output stream wet;

    external ir: AudioAsset;

    nodes {
        reverb = Convolver::new();
    }

    connections {
        reverb.output -> wet;
        ir -> reverb.typo;
    }
}

fn main() {}
