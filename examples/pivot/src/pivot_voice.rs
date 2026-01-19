use crate::fm_operator::FmOperator;
use crate::vca::Vca;
use oscen::prelude::*;

// 3-operator FM voice: OP3 -> OP2 -> OP1 -> Filter
// Simplified chain without routing crossfade
graph! {
    name: PivotVoice;

    // Voice inputs
    input frequency: value = 440.0;
    input gate: event;

    // OP3 (top modulator)
    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5;
    input op3_feedback: value = 0.0;

    // OP2 (middle modulator)
    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5;
    input op2_feedback: value = 0.0;

    // OP1 (carrier)
    input op1_ratio: value = 1.0;
    input op1_feedback: value = 0.0;

    // Filter
    input cutoff: value = 2000.0;
    input resonance: value = 0.707;

    output audio_out: stream;

    nodes {
        // Operator envelopes
        env3 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env2 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env1 = AdsrEnvelope::new(0.01, 0.2, 0.8, 0.5);

        // Operators
        op3_osc = FmOperator::new();
        op2_osc = FmOperator::new();
        op1_osc = FmOperator::new();

        // VCA nodes for envelope modulation (stream × stream)
        op3_env_vca = Vca::new();
        op2_env_vca = Vca::new();
        op1_env_vca = Vca::new();

        // Gain nodes for level control (stream × value)
        op3_level_gain = Gain::new(0.5);
        op2_level_gain = Gain::new(0.5);

        // Filter
        filter = TptFilter::new(2000.0, 0.707);

        // Output gain
        output_gain = Gain::new(0.3);
    }

    connections {
        // Gate to all envelopes
        gate -> env3.gate;
        gate -> env2.gate;
        gate -> env1.gate;

        // OP3: osc -> env_vca -> level_gain -> OP2.phase_mod
        frequency -> op3_osc.base_freq;
        op3_ratio -> op3_osc.ratio;
        op3_feedback -> op3_osc.feedback;
        op3_osc.output -> op3_env_vca.input;
        env3.output -> op3_env_vca.control;
        op3_env_vca.output -> op3_level_gain.input;
        op3_level -> op3_level_gain.gain;
        op3_level_gain.output -> op2_osc.phase_mod;

        // OP2: osc -> env_vca -> level_gain -> OP1.phase_mod
        frequency -> op2_osc.base_freq;
        op2_ratio -> op2_osc.ratio;
        op2_feedback -> op2_osc.feedback;
        op2_osc.output -> op2_env_vca.input;
        env2.output -> op2_env_vca.control;
        op2_env_vca.output -> op2_level_gain.input;
        op2_level -> op2_level_gain.gain;
        op2_level_gain.output -> op1_osc.phase_mod;

        // OP1 (carrier): osc -> env_vca -> filter
        frequency -> op1_osc.base_freq;
        op1_ratio -> op1_osc.ratio;
        op1_feedback -> op1_osc.feedback;
        op1_osc.output -> op1_env_vca.input;
        env1.output -> op1_env_vca.control;
        op1_env_vca.output -> filter.input;

        // Filter parameters
        cutoff -> filter.cutoff;
        resonance -> filter.q;

        // Final output
        filter.output -> output_gain.input;
        output_gain.output -> audio_out;
    }
}
