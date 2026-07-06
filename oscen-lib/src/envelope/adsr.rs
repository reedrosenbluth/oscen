use crate::graph::types::EventPayload;
use crate::graph::{EventInput, EventInstance, SampleRate, SignalProcessor};
use crate::Node;

const MIN_TIME_SECONDS: f32 = 1.0e-5;
// One-pole curve target: at the end of an attack/decay stage, the level is
// 1 - 1% = 99% of the way to the target, then snapped. -ln(0.01) ≈ 4.605.
const CURVE_TIME_CONSTANT: f32 = 4.605_170_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Node)]
pub struct AdsrEnvelope {
    #[input(event)]
    pub gate: EventInput,

    #[input(value)]
    pub attack: f32,

    #[input(value)]
    pub decay: f32,

    #[input(value)]
    pub sustain: f32,

    #[input(value)]
    pub release: f32,

    /// How much note velocity scales the envelope: 1.0 = full velocity
    /// scaling, 0.0 = velocity-insensitive. Read at gate time only; mid-note
    /// changes take effect on the next gate.
    #[input(value)]
    pub velocity_amount: f32,

    #[output(stream)]
    pub output: f32,

    stage: Stage,
    attack_samples: u32,
    decay_samples: u32,
    release_samples: u32,
    samples_remaining: u32,
    // Per-sample coefficients for the one-pole approach used by Attack (toward
    // `peak`) and Decay (toward sustain_level). Release is still linear and
    // uses `release_increment`.
    attack_coeff: f32,
    decay_coeff: f32,
    release_increment: f32,
    level: f32,
    target_level: f32,
    sustain_level: f32,
    // Per-note peak the attack targets, computed from velocity and
    // `velocity_amount` at gate time.
    peak: f32,
    sample_rate: SampleRate,
}

impl AdsrEnvelope {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let mut envelope = Self {
            gate: EventInput::default(),
            attack,
            decay,
            sustain,
            release,
            velocity_amount: 1.0,
            output: 0.0,
            stage: Stage::Idle,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            samples_remaining: 0,
            attack_coeff: 0.0,
            decay_coeff: 0.0,
            release_increment: 0.0,
            level: 0.0,
            target_level: 0.0,
            sustain_level: sustain.clamp(0.0, 1.0),
            peak: 1.0,
            sample_rate: SampleRate::default(),
        };
        envelope.update_sustain_level();
        envelope
    }

    fn apply_parameters(&mut self) {
        self.attack = self.attack.max(0.0);
        self.decay = self.decay.max(0.0);
        self.sustain = self.sustain.clamp(0.0, 1.0);
        self.release = self.release.max(0.0);
        self.update_sustain_level();
    }

    fn update_sustain_level(&mut self) {
        self.sustain_level = (self.sustain * self.peak).clamp(0.0, 1.0);
        let old_attack_samples = self.attack_samples;
        let old_decay_samples = self.decay_samples;
        let old_release_samples = self.release_samples;
        self.recalculate_cached_steps();
        // If the active stage's total length changed, rescale the remaining
        // sample count so the stage keeps its fractional progress. The cached
        // coefficients are derived from the new total, so keeping the old
        // remaining count would end the stage far from its target and snap.
        let (old_total, new_total) = match self.stage {
            Stage::Attack if self.samples_remaining > 0 => {
                (old_attack_samples, self.attack_samples)
            }
            Stage::Decay if self.samples_remaining > 0 => (old_decay_samples, self.decay_samples),
            Stage::Release if self.samples_remaining > 0 => {
                (old_release_samples, self.release_samples)
            }
            _ => (0, 0),
        };
        if old_total > 0 && old_total != new_total {
            let rescaled = new_total as u64 * self.samples_remaining as u64 / old_total as u64;
            self.samples_remaining = (rescaled as u32).max(1);
        }
        match self.stage {
            Stage::Decay | Stage::Sustain => self.target_level = self.sustain_level,
            Stage::Release => self.target_level = 0.0,
            _ => {}
        }
        if matches!(self.stage, Stage::Release) {
            self.update_release_increment();
        }
    }

    fn recalculate_cached_steps(&mut self) {
        let sample_rate = self.sample_rate.max(1.0);

        self.attack_samples = (self.attack.max(MIN_TIME_SECONDS) * sample_rate) as u32;
        self.attack_samples = self.attack_samples.max(1);

        self.decay_samples = (self.decay.max(MIN_TIME_SECONDS) * sample_rate) as u32;
        self.decay_samples = self.decay_samples.max(1);

        self.release_samples = (self.release.max(MIN_TIME_SECONDS) * sample_rate) as u32;
        self.release_samples = self.release_samples.max(1);

        // One-pole coefficient: after `n` samples of `level += (target - level) * c`,
        // level is `1 - (1 - c)^n` of the way to target. Picking
        // `c = 1 - exp(-K/n)` makes that fraction `1 - exp(-K) ≈ 99%` at stage end.
        self.attack_coeff = 1.0 - (-CURVE_TIME_CONSTANT / self.attack_samples as f32).exp();
        self.decay_coeff = 1.0 - (-CURVE_TIME_CONSTANT / self.decay_samples as f32).exp();
    }

    fn set_stage(&mut self, stage: Stage, target_level: f32) {
        self.stage = stage;
        self.target_level = target_level.clamp(0.0, 1.0);

        let samples = match stage {
            Stage::Attack => self.attack_samples,
            Stage::Decay => self.decay_samples,
            Stage::Release => self.release_samples,
            Stage::Sustain | Stage::Idle => 0,
        };

        if samples == 0 {
            self.samples_remaining = 0;
            self.release_increment = 0.0;
            self.level = self.target_level;
            if !matches!(stage, Stage::Sustain | Stage::Idle) {
                self.complete_stage();
            }
        } else {
            self.samples_remaining = samples;
            self.update_release_increment();
        }
    }

    fn update_release_increment(&mut self) {
        // Release stays linear: per-sample slope that lands at zero after
        // `samples_remaining` samples.
        if self.samples_remaining == 0 || !matches!(self.stage, Stage::Release) {
            self.release_increment = 0.0;
            return;
        }
        let current = self.level.clamp(0.0, 1.0);
        self.release_increment = if current <= 0.0 {
            0.0
        } else {
            -current / self.samples_remaining as f32
        };
    }

    fn complete_stage(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level = self.peak;
                self.set_stage(Stage::Decay, self.sustain_level);
            }
            Stage::Decay => {
                self.level = self.sustain_level;
                self.stage = Stage::Sustain;
                self.samples_remaining = 0;
                self.release_increment = 0.0;
            }
            Stage::Release => {
                self.level = 0.0;
                self.stage = Stage::Idle;
                self.samples_remaining = 0;
                self.release_increment = 0.0;
            }
            Stage::Sustain => {
                self.level = self.sustain_level;
                self.samples_remaining = 0;
                self.release_increment = 0.0;
            }
            Stage::Idle => {
                self.level = 0.0;
                self.samples_remaining = 0;
                self.release_increment = 0.0;
            }
        }
    }

    fn process_stage(&mut self) {
        match self.stage {
            Stage::Attack => {
                if self.samples_remaining > 0 {
                    self.level += (self.peak - self.level) * self.attack_coeff;
                    self.samples_remaining -= 1;
                    self.level = self.level.clamp(0.0, 1.0);
                }
                if self.samples_remaining == 0 {
                    self.level = self.peak;
                    self.complete_stage();
                }
            }
            Stage::Decay => {
                if self.samples_remaining > 0 {
                    self.level += (self.sustain_level - self.level) * self.decay_coeff;
                    self.samples_remaining -= 1;
                    self.level = self.level.clamp(0.0, 1.0);
                }
                if self.samples_remaining == 0 {
                    self.level = self.sustain_level;
                    self.complete_stage();
                }
            }
            Stage::Release => {
                if self.samples_remaining > 0 {
                    self.level += self.release_increment;
                    self.samples_remaining -= 1;
                    self.level = self.level.clamp(0.0, 1.0);
                }
                if self.samples_remaining == 0 {
                    self.level = 0.0;
                    self.complete_stage();
                }
            }
            Stage::Sustain => {
                self.level = self.sustain_level;
            }
            Stage::Idle => {
                self.level = 0.0;
            }
        }
    }

    fn handle_gate_event(&mut self, event: &EventInstance) {
        let velocity = match &event.payload {
            EventPayload::Scalar(v) => *v,
            EventPayload::Object(_) => 1.0,
        };

        if velocity > 0.0 {
            let amount = self.velocity_amount.clamp(0.0, 1.0);
            let velocity = velocity.clamp(0.0, 1.0);
            self.peak = (1.0 - amount + amount * velocity).clamp(0.0, 1.0);
            self.update_sustain_level();
            if self.attack <= MIN_TIME_SECONDS {
                self.level = self.peak;
                self.set_stage(Stage::Decay, self.sustain_level);
            } else {
                self.set_stage(Stage::Attack, self.peak);
            }
        } else if self.release <= MIN_TIME_SECONDS {
            self.stage = Stage::Idle;
            self.level = 0.0;
            self.samples_remaining = 0;
            self.release_increment = 0.0;
        } else {
            self.set_stage(Stage::Release, 0.0);
        }
    }
}

impl SignalProcessor for AdsrEnvelope {
    fn prepare(&mut self) {
        self.update_sustain_level();
    }

    #[inline(always)]
    fn process(&mut self) {
        // Apply parameters from struct fields
        self.apply_parameters();

        // Process envelope stage
        self.process_stage();

        // Update output level
        self.output = self.level;
    }

    fn is_active(&self) -> bool {
        // Envelope is inactive only when idle and level is zero
        // We still process during Sustain stage even though it's static,
        // since we need to handle gate-off events
        !matches!(self.stage, Stage::Idle) || self.level > 0.0
    }
}

impl AdsrEnvelope {
    // Event handler called automatically by derive macro via process_event_inputs()
    fn on_gate(&mut self, event: &EventInstance) {
        self.handle_gate_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EventPayload;
    use float_cmp::approx_eq;

    #[test]
    fn attack_reaches_target_on_schedule() {
        let sample_rate = 48_000.0;
        let attack_seconds = 0.05;
        let mut env = AdsrEnvelope::new(attack_seconds, 0.1, 0.5, 0.05);
        env.set_sample_rate(sample_rate);
        env.prepare();

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });

        // The one-pole coefficient is derived so the level is 99% of the way
        // to the target on the last attack sample, where it snaps to 1.0.
        let attack_samples = (attack_seconds * sample_rate) as u32;
        for _ in 0..attack_samples - 1 {
            env.process();
        }
        assert!(
            approx_eq!(f32, env.output, 0.99, epsilon = 0.001),
            "level {} not ~99% of target one sample before attack end",
            env.output
        );

        env.process();
        assert!(
            approx_eq!(f32, env.output, 1.0, ulps = 2),
            "level {} did not reach target at attack end",
            env.output
        );
    }

    #[test]
    fn reaches_sustain_level() {
        let mut env = AdsrEnvelope::new(0.01, 0.02, 0.6, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();

        // Trigger gate on
        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });

        for _ in 0..4_800 {
            env.process();
        } // 100 ms

        assert!(
            env.output >= 0.5 && env.output <= 0.65,
            "value {} not near sustain",
            env.output
        );
    }

    #[test]
    fn release_returns_to_zero() {
        let mut env = AdsrEnvelope::new(0.0, 0.0, 0.8, 0.01);
        env.set_sample_rate(48_000.0);
        env.prepare();

        // Trigger gate on
        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });

        for _ in 0..100 {
            env.process();
        }

        // Trigger gate off
        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.0),
        });

        for _ in 0..4_800 {
            env.process();
        }

        assert!(env.output <= 0.01, "value {} not near zero", env.output);
    }

    #[test]
    fn lengthening_attack_mid_stage_does_not_snap() {
        let mut env = AdsrEnvelope::new(0.01, 0.02, 0.6, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });

        // 100 samples into the 0.01 s attack, lengthen it to 1.0 s.
        for _ in 0..100 {
            env.process();
        }
        env.attack = 1.0;

        // Run through the rest of the attack and into the decay; the largest
        // single-sample step must stay small (no amplitude snap). The one-pole
        // curve lands 99% of the way to its target before snapping, so the
        // stage-end correction is at most ~0.01.
        let mut previous = env.output;
        let mut max_step = 0.0f32;
        for _ in 0..96_000 {
            env.process();
            max_step = max_step.max((env.output - previous).abs());
            previous = env.output;
        }

        assert!(
            max_step < 0.011,
            "attack change caused amplitude snap of {max_step}"
        );
    }

    #[test]
    fn velocity_scales_output() {
        let mut env = AdsrEnvelope::new(0.0, 0.0, 1.0, 0.01);
        env.set_sample_rate(48_000.0);
        env.prepare();

        // Trigger gate with 0.5 velocity
        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.5),
        });

        for _ in 0..100 {
            env.process();
        }

        assert!(
            env.output >= 0.45 && env.output <= 0.55,
            "value {} not scaled by velocity",
            env.output
        );
    }

    #[test]
    fn soft_note_peaks_at_velocity() {
        let mut env = AdsrEnvelope::new(0.01, 0.02, 0.6, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.25),
        });

        let mut max_output = 0.0f32;
        for _ in 0..4_800 {
            env.process();
            max_output = max_output.max(env.output);
        }

        assert!(
            approx_eq!(f32, max_output, 0.25, epsilon = 0.001),
            "peak {max_output} not scaled by velocity"
        );
        assert!(
            approx_eq!(f32, env.output, 0.25 * 0.6, epsilon = 0.005),
            "sustain {} not velocity * sustain",
            env.output
        );
    }

    #[test]
    fn amount_zero_ignores_velocity() {
        let mut env = AdsrEnvelope::new(0.01, 0.05, 0.6, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();
        env.velocity_amount = 0.0;

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.2),
        });

        let mut max_output = 0.0f32;
        for _ in 0..600 {
            env.process();
            max_output = max_output.max(env.output);
        }

        assert!(
            approx_eq!(f32, max_output, 1.0, ulps = 2),
            "peak {max_output} did not reach 1.0 with velocity_amount 0"
        );
    }

    #[test]
    fn half_amount_half_velocity_peaks_at_three_quarters() {
        let mut env = AdsrEnvelope::new(0.01, 0.05, 0.6, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();
        env.velocity_amount = 0.5;

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.5),
        });

        let mut max_output = 0.0f32;
        for _ in 0..600 {
            env.process();
            max_output = max_output.max(env.output);
        }

        assert!(
            approx_eq!(f32, max_output, 0.75, epsilon = 0.001),
            "peak {max_output} not 1 - amount + amount * velocity"
        );
    }

    #[test]
    fn retrigger_at_lower_velocity_descends_smoothly() {
        let mut env = AdsrEnvelope::new(0.05, 0.02, 0.8, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });
        for _ in 0..4_800 {
            env.process();
        }
        assert!(
            approx_eq!(f32, env.output, 0.8, epsilon = 0.001),
            "level {} not at sustain before retrigger",
            env.output
        );

        // Retrigger softer while the level is high: the attack must move
        // down toward the new peak with no upward jump and no snap.
        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.25),
        });

        let mut previous = env.output;
        let mut max_step = 0.0f32;
        for _ in 0..2_400 {
            env.process();
            assert!(
                env.output <= previous + 1.0e-6,
                "level moved up from {previous} to {} on soft retrigger",
                env.output
            );
            max_step = max_step.max((previous - env.output).abs());
            previous = env.output;
        }
        assert!(
            approx_eq!(f32, env.output, 0.25, epsilon = 0.001),
            "level {} not at new peak after retriggered attack",
            env.output
        );
        assert!(
            max_step < 0.011,
            "retrigger caused amplitude snap of {max_step}"
        );

        // Decay then settles at the velocity-scaled sustain.
        for _ in 0..2_400 {
            env.process();
        }
        assert!(
            approx_eq!(f32, env.output, 0.8 * 0.25, epsilon = 0.001),
            "sustain {} not rescaled by retrigger velocity",
            env.output
        );
    }

    #[test]
    fn instant_attack_retrigger_snaps_to_new_peak() {
        let mut env = AdsrEnvelope::new(0.0, 0.02, 1.0, 0.05);
        env.set_sample_rate(48_000.0);
        env.prepare();

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(1.0),
        });
        for _ in 0..2_000 {
            env.process();
        }
        assert!(approx_eq!(f32, env.output, 1.0, ulps = 2));

        env.handle_gate_event(&EventInstance {
            frame_offset: 0,
            payload: EventPayload::scalar(0.5),
        });
        for _ in 0..100 {
            env.process();
            assert!(
                env.output <= 0.5 + 1.0e-6,
                "level {} exceeded new peak after instant-attack retrigger",
                env.output
            );
        }
        assert!(
            approx_eq!(f32, env.output, 0.5, epsilon = 0.001),
            "level {} not at new peak",
            env.output
        );
    }
}
