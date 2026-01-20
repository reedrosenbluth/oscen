use crate::graph::types::EventPayload;
use crate::graph::{
    EventInput, EventInstance, InputEndpoint, NodeKey, ProcessingNode, SignalProcessor, ValueKey,
};
use crate::Node;

const MIN_TIME_SECONDS: f32 = 1.0e-5;

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

    #[output(stream)]
    pub output: f32,

    stage: Stage,
    attack_samples: u32,
    decay_samples: u32,
    release_samples: u32,
    samples_remaining: u32,
    increment: f32,
    level: f32,
    target_level: f32,
    sustain_level: f32,
    velocity: f32,
    sample_rate: f32,
}

impl AdsrEnvelope {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let mut envelope = Self {
            gate: EventInput::default(),
            attack,
            decay,
            sustain,
            release,
            output: 0.0,
            stage: Stage::Idle,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            samples_remaining: 0,
            increment: 0.0,
            level: 0.0,
            target_level: 0.0,
            sustain_level: sustain.clamp(0.0, 1.0),
            velocity: 1.0,
            sample_rate: 44_100.0,
        };
        envelope.update_sustain_level();
        envelope
    }

    fn apply_parameters(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.attack = attack.max(0.0);
        self.decay = decay.max(0.0);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release = release.max(0.0);
        self.update_sustain_level();
    }

    fn update_sustain_level(&mut self) {
        self.sustain_level = (self.sustain * self.velocity).clamp(0.0, 1.0);
        self.recalculate_cached_steps();
        match self.stage {
            Stage::Attack if self.samples_remaining > 0 => {
                self.samples_remaining = self.samples_remaining.min(self.attack_samples).max(1);
            }
            Stage::Decay if self.samples_remaining > 0 => {
                self.samples_remaining = self.samples_remaining.min(self.decay_samples).max(1);
            }
            Stage::Release if self.samples_remaining > 0 => {
                self.samples_remaining = self.samples_remaining.min(self.release_samples).max(1);
            }
            _ => {}
        }
        match self.stage {
            Stage::Decay | Stage::Sustain => self.target_level = self.sustain_level,
            Stage::Release => self.target_level = 0.0,
            _ => {}
        }
        if matches!(self.stage, Stage::Attack | Stage::Decay | Stage::Release) {
            self.update_increment_for_stage();
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
    }

    fn set_stage(&mut self, stage: Stage, _duration_secs: f32, target_level: f32) {
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
            self.increment = 0.0;
            self.level = self.target_level;
            if !matches!(stage, Stage::Sustain | Stage::Idle) {
                self.complete_stage();
            }
        } else {
            self.samples_remaining = samples;
            self.update_increment_for_stage();
        }
    }

    fn update_increment_for_stage(&mut self) {
        if self.samples_remaining == 0 {
            self.increment = 0.0;
            return;
        }

        let current = self.level.clamp(0.0, 1.0);

        self.increment = match self.stage {
            Stage::Attack => {
                let delta = (1.0 - current).max(0.0);
                delta / self.samples_remaining as f32
            }
            Stage::Decay => {
                let delta = self.sustain_level - current;
                delta / self.samples_remaining as f32
            }
            Stage::Release => {
                if current <= 0.0 {
                    0.0
                } else {
                    -current / self.samples_remaining as f32
                }
            }
            Stage::Sustain | Stage::Idle => 0.0,
        };
    }

    fn complete_stage(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level = 1.0;
                self.set_stage(Stage::Decay, self.decay, self.sustain_level);
            }
            Stage::Decay => {
                self.level = self.sustain_level;
                self.stage = Stage::Sustain;
                self.samples_remaining = 0;
                self.increment = 0.0;
            }
            Stage::Release => {
                self.level = 0.0;
                self.stage = Stage::Idle;
                self.samples_remaining = 0;
                self.increment = 0.0;
            }
            Stage::Sustain => {
                self.level = self.sustain_level;
                self.samples_remaining = 0;
                self.increment = 0.0;
            }
            Stage::Idle => {
                self.level = 0.0;
                self.samples_remaining = 0;
                self.increment = 0.0;
            }
        }
    }

    fn process_stage(&mut self) {
        match self.stage {
            Stage::Attack | Stage::Decay | Stage::Release => {
                if self.samples_remaining > 0 {
                    self.level += self.increment;
                    self.samples_remaining -= 1;
                    self.level = self.level.clamp(0.0, 1.0);
                }

                if self.samples_remaining == 0 {
                    self.level = self.target_level;
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
            self.velocity = velocity.clamp(0.0, 1.0);
            self.update_sustain_level();
            if self.attack <= MIN_TIME_SECONDS {
                self.level = 1.0;
                self.set_stage(Stage::Decay, self.decay, self.sustain_level);
            } else {
                self.set_stage(Stage::Attack, self.attack.max(MIN_TIME_SECONDS), 1.0);
            }
        } else if self.release <= MIN_TIME_SECONDS {
            self.stage = Stage::Idle;
            self.level = 0.0;
            self.samples_remaining = 0;
            self.increment = 0.0;
        } else {
            self.set_stage(Stage::Release, self.release.max(MIN_TIME_SECONDS), 0.0);
        }
    }
}

impl SignalProcessor for AdsrEnvelope {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_sustain_level();
    }

    #[inline(always)]
    fn process(&mut self) {
        // Apply parameters from struct fields
        self.apply_parameters(self.attack, self.decay, self.sustain, self.release);

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
    // Event handler called automatically by the macro-generated NodeIO
    fn on_gate(&mut self, event: &EventInstance) {
        self.handle_gate_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EventPayload;

    #[test]
    fn reaches_sustain_level() {
        let mut env = AdsrEnvelope::new(0.01, 0.02, 0.6, 0.05);
        env.init(48_000.0);

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
        env.init(48_000.0);

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
    fn velocity_scales_output() {
        let mut env = AdsrEnvelope::new(0.0, 0.0, 1.0, 0.01);
        env.init(48_000.0);

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
}
