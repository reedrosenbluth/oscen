use oscen_macros::graph;

#[test]
fn test_just_range() {
    graph! {
        input value cutoff = 3000.0 [range(20.0, 20000.0)];
        output stream out;
    }
}

#[test]
fn test_just_log() {
    graph! {
        input value cutoff = 3000.0 [log];
        output stream out;
    }
}

#[test]
fn test_range_and_log() {
    graph! {
        input value cutoff = 3000.0 [range(20.0, 20000.0), log];
        output stream out;
    }
}
