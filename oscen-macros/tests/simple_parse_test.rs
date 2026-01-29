use oscen_macros::graph;
// Import oscen for the macro to work
use oscen as _;

#[test]
fn test_just_range() {
    graph! {
        name: TestJustRangeGraph;
        input value cutoff = 3000.0 [20.0..20000.0];
        output stream out;
    }
}

#[test]
fn test_just_log() {
    graph! {
        name: TestJustLogGraph;
        input value cutoff = 3000.0 [log];
        output stream out;
    }
}

#[test]
fn test_range_and_log() {
    graph! {
        name: TestRangeAndLogGraph;
        input value cutoff = 3000.0 [20.0..20000.0, log];
        output stream out;
    }
}
