//! Fan-in into a nested graph's endpoints. The parent cannot see a child
//! graph's endpoint kinds, so a multi-driver destination is checked by rustc
//! through the child's `EndpointAt` marker: a stream input sums its drivers
//! (and a value input is rejected — see
//! `oscen-macros/tests/ui/nested_graph_value_fanin.rs`).
//!
//! Deliberately compiled without `#![feature(inherent_associated_types)]`:
//! graph-emitted markers and the fan-in assertion must not need it.

use oscen::graph;

graph! {
    name: Passthrough;
    input stream input;
    output stream output;
    connections { input -> output; }
}

graph! {
    name: SumIntoChild;
    input stream a;
    input stream b;
    output stream out;
    nodes {
        // Nested-graph sources: the parent knows neither side's kind, so the
        // fan-in is checked through `Passthrough`'s `EndpointAt` marker.
        pa = Passthrough::new();
        pb = Passthrough::new();
        child = Passthrough::new();
    }
    connections {
        a -> pa.input;
        b -> pb.input;
        pa.output -> child.input;
        pb.output -> child.input;
        child.output -> out;
    }
}

#[test]
fn nested_graph_stream_input_sums_its_drivers() {
    let mut graph = SumIntoChild::new();
    graph.init(48_000.0);
    graph.a = 0.25;
    graph.b = 0.5;
    graph.process();
    assert_eq!(graph.out, 0.75);

    graph.a_block[..4].copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
    graph.b_block[..4].copy_from_slice(&[0.5; 4]);
    graph.process_block(4);
    assert_eq!(graph.out_block[..4], [1.5, 2.5, 3.5, 4.5]);
}

#[test]
fn graph_endpoints_expose_endpoint_at_markers() {
    fn kind_of<N: oscen::dispatch::EndpointAt<M>, M>(
        _: fn() -> core::marker::PhantomData<M>,
    ) -> &'static str {
        core::any::type_name::<<N as oscen::dispatch::EndpointAt<M>>::Kind>()
    }
    assert!(kind_of::<Passthrough, _>(Passthrough::input__ep).ends_with("StreamKind"));
    assert!(kind_of::<Passthrough, _>(Passthrough::output__ep).ends_with("StreamKind"));
}
