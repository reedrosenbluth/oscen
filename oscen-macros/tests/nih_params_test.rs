/// Test that the nih_params feature generates correct code.
/// This test is only compiled when the nih-plug feature is enabled.

#[cfg(feature = "nih-plug")]
mod with_feature {
    use nih_plug::prelude::*;
    use oscen::prelude::*;
    use oscen_macros::graph;

    graph! {
        name: TestGraph;
        nih_params;

        // Compact bracket syntax with range, skew, and options
        input cutoff: value = 2000.0 [20.0..20000.0 @ -2.0, unit = "Hz", name = "Filter Cutoff"];

        // Bracket syntax with step size
        input ratio: value = 1.0 [0.5..16.0, step = 0.5];

        // Simple range only
        input level: value = 0.5 [0.0..2.0];

        // No spec = 0..1 range with default smoothing
        input simple_param: value = 0.5;

        output out: stream;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.5);
        }

        connections {
            osc.output -> out;
        }
    }

    #[test]
    fn test_params_struct_exists() {
        // This will fail to compile if TestGraphParams doesn't exist
        let _params = TestGraphParams::default();
    }

    #[test]
    fn test_sync_to_method_exists() {
        let params = TestGraphParams::default();
        let mut graph = TestGraph::new();
        params.sync_to(&mut graph);
    }

    #[test]
    fn test_param_fields() {
        let params = TestGraphParams::default();
        // Check that fields exist and have expected defaults
        assert!((params.cutoff.value() - 2000.0).abs() < 0.001);
        assert!((params.ratio.value() - 1.0).abs() < 0.001);
        assert!((params.level.value() - 0.5).abs() < 0.001);
        assert!((params.simple_param.value() - 0.5).abs() < 0.001);
    }
}

/// Test that without nih_params, no params struct is generated.
mod without_nih_params {
    use oscen::prelude::*;
    use oscen_macros::graph;

    graph! {
        name: SimpleGraph;

        input volume: value = 1.0;
        output out: stream;

        nodes {
            osc = PolyBlepOscillator::saw(440.0, 0.5);
        }

        connections {
            osc.output -> out;
        }
    }

    #[test]
    fn test_graph_works_without_nih_params() {
        let mut graph = SimpleGraph::new();
        graph.init(44100.0);
        graph.volume = 0.8;
        graph.process();
    }
}
