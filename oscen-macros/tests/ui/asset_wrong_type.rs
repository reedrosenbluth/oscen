// An `external` must carry an asset type (`AudioAsset`). A foreign type is
// rejected at the declaration rather than ignored.

use oscen::graph;
use oscen::prelude::*;

pub struct NotAnAsset;

graph! {
    name: AssetWrongType;

    input stream dry;
    output stream wet;

    external ir: NotAnAsset;

    nodes {
        reverb = Convolver::new();
    }

    connections {
        dry -> reverb.input;
        reverb.output -> wet;
        ir -> reverb.ir;
    }
}

fn main() {}
