use crate::nodes::{AddValue, Crossfade, FmOperator, Mixer};
use oscen::prelude::*;

// 3-operator FM voice with routing crossfade
// Route parameter blends OP3's modulation between OP2 (route=0) and OP1 (route=1)
graph! {
    name: FMVoice;

    // Voice inputs
    input frequency: value = 440.0;
    input gate: event;

    // OP3 (top modulator)
    input op3_ratio: value = 3.0;
    input op3_level: value = 0.5;
    input op3_feedback: value = 0.0;
    input op3_attack: value = 0.01;
    input op3_decay: value = 0.1;
    input op3_sustain: value = 0.7;
    input op3_release: value = 0.3;

    // OP2 (middle modulator)
    input op2_ratio: value = 2.0;
    input op2_level: value = 0.5;
    input op2_feedback: value = 0.0;
    input op2_attack: value = 0.01;
    input op2_decay: value = 0.1;
    input op2_sustain: value = 0.7;
    input op2_release: value = 0.3;

    // OP1 (carrier) - ratio always 1.0, no feedback
    input op1_ratio: value = 1.0;
    input op1_attack: value = 0.01;
    input op1_decay: value = 0.2;
    input op1_sustain: value = 0.8;
    input op1_release: value = 0.5;

    // Route: 0.0 = OP3->OP2, 1.0 = OP3->OP1
    input route: value = 0.0;

    // Filter
    input filter_cutoff: value = 2000.0;
    input filter_resonance: value = 0.707;
    input filter_attack: value = 0.01;
    input filter_decay: value = 0.2;
    input filter_sustain: value = 0.5;
    input filter_release: value = 0.3;
    input filter_env_amount: value = 0.0;  // in Hz

    output audio_out: stream;

    nodes {
        // Operator envelopes
        env3 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env2 = AdsrEnvelope::new(0.01, 0.1, 0.7, 0.3);
        env1 = AdsrEnvelope::new(0.01, 0.2, 0.8, 0.5);

        // Filter envelope
        env_filter = AdsrEnvelope::new(0.01, 0.2, 0.5, 0.3);
        filter_env_gain = Gain::new(0.0);  // scales envelope by env_amount
        cutoff_mod = AddValue::new(2000.0);  // adds base cutoff to envelope mod

        // Operators (with integrated envelope and level)
        op3_osc = FmOperator::new();
        op2_osc = FmOperator::new();
        op1_osc = FmOperator::new();

        // Crossfade for OP3 routing: output_a -> OP2, output_b -> OP1
        op3_route = Crossfade::new();

        // Mixer to combine OP2 output + routed OP3 for OP1's phase_mod
        op1_mod_mixer = Mixer::new();

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
        gate -> env_filter.gate;

        // OP3 envelope parameters
        op3_attack -> env3.attack;
        op3_decay -> env3.decay;
        op3_sustain -> env3.sustain;
        op3_release -> env3.release;

        // OP2 envelope parameters
        op2_attack -> env2.attack;
        op2_decay -> env2.decay;
        op2_sustain -> env2.sustain;
        op2_release -> env2.release;

        // OP1 envelope parameters
        op1_attack -> env1.attack;
        op1_decay -> env1.decay;
        op1_sustain -> env1.sustain;
        op1_release -> env1.release;

        // Filter envelope parameters
        filter_attack -> env_filter.attack;
        filter_decay -> env_filter.decay;
        filter_sustain -> env_filter.sustain;
        filter_release -> env_filter.release;

        // Filter envelope modulation: env -> gain(amount) -> add(cutoff) -> filter
        env_filter.output -> filter_env_gain.input;
        filter_env_amount -> filter_env_gain.gain;
        filter_env_gain.output -> cutoff_mod.input;
        filter_cutoff -> cutoff_mod.value;
        cutoff_mod.output -> filter.cutoff;

        // OP3: envelope and level integrated into operator
        frequency -> op3_osc.base_freq;
        op3_ratio -> op3_osc.ratio;
        op3_feedback -> op3_osc.feedback;
        env3.output -> op3_osc.envelope;
        op3_level -> op3_osc.level;

        // Route crossfade: splits OP3 between OP2 (output_a) and OP1 (output_b)
        op3_osc.output -> op3_route.input;
        route -> op3_route.mix;
        op3_route.output_a -> op2_osc.phase_mod;  // OP3 -> OP2 when route=0

        // OP2: envelope and level integrated into operator
        frequency -> op2_osc.base_freq;
        op2_ratio -> op2_osc.ratio;
        op2_feedback -> op2_osc.feedback;
        env2.output -> op2_osc.envelope;
        op2_level -> op2_osc.level;

        // Mix OP2 output + routed OP3 (output_b) for OP1's phase modulation
        op2_osc.output -> op1_mod_mixer.input_a;
        op3_route.output_b -> op1_mod_mixer.input_b;  // OP3 -> OP1 when route=1
        op1_mod_mixer.output -> op1_osc.phase_mod;

        // OP1 (carrier): envelope integrated into operator
        frequency -> op1_osc.base_freq;
        op1_ratio -> op1_osc.ratio;
        env1.output -> op1_osc.envelope;
        op1_osc.output -> filter.input;

        // Filter resonance
        filter_resonance -> filter.q;

        // Final output
        filter.output -> output_gain.input;
        output_gain.output -> audio_out;
    }
}
