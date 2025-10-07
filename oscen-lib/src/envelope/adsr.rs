use crate::graph::types::EventPayload;
use crate::graph::{
    EventInstance, InputEndpoint, NodeKey, ProcessingContext, ProcessingNode,
    SignalProcessor, ValueKey,
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
    gate: (),

    #[input(value)]
    attack: f32,

    #[input(value)]
    decay: f32,

    #[input(value)]
    sustain: f32,

    #[input(value)]
    release: f32,

    #[output(stream)]
    output: f32,

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
            gate: (),
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

    fn process<'a>(&mut self, _sample_rate: f32, context: &mut ProcessingContext<'a>) -> f32 {
        let attack = self.get_attack(context);
        let decay = self.get_decay(context);
        let sustain = self.get_sustain(context);
        let release = self.get_release(context);
        self.apply_parameters(attack, decay, sustain, release);

        for event in self.events_gate(&*context).iter() {
            self.handle_gate_event(event);
        }

        self.process_stage();

        self.output = self.level;
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EventPayload;
    use crate::graph::Graph;

    #[test]
    fn reaches_sustain_level() {
        let mut graph = Graph::new(48_000.0);
        let env = graph.add_node(AdsrEnvelope::new(0.01, 0.02, 0.6, 0.05));

        let _ = graph
            .insert_value_input(env.attack(), 0.01)
            .expect("attack input available");
        let _ = graph
            .insert_value_input(env.decay(), 0.02)
            .expect("decay input available");
        let _ = graph
            .insert_value_input(env.sustain(), 0.6)
            .expect("sustain input available");
        let _ = graph
            .insert_value_input(env.release(), 0.05)
            .expect("release input available");

        graph.queue_event(env.gate(), 0, EventPayload::scalar(1.0));
        for _ in 0..4_800 {
            graph.process().expect("graph processes");
        } // 100 ms

        let value = graph.get_value(&env.output()).unwrap();
        assert!(
            value >= 0.5 && value <= 0.65,
            "value {} not near sustain",
            value
        );
    }

    #[test]
    fn release_returns_to_zero() {
        let mut graph = Graph::new(48_000.0);
        let env = graph.add_node(AdsrEnvelope::new(0.0, 0.0, 0.8, 0.01));

        let _ = graph
            .insert_value_input(env.attack(), 0.0)
            .expect("attack input available");
        let _ = graph
            .insert_value_input(env.decay(), 0.0)
            .expect("decay input available");
        let _ = graph
            .insert_value_input(env.sustain(), 0.8)
            .expect("sustain input available");
        let _ = graph
            .insert_value_input(env.release(), 0.01)
            .expect("release input available");

        graph.queue_event(env.gate(), 0, EventPayload::scalar(1.0));
        for _ in 0..100 {
            graph.process().expect("graph processes on note on");
        }
        graph.queue_event(env.gate(), 0, EventPayload::scalar(0.0));
        for _ in 0..4_800 {
            graph.process().expect("graph processes on release");
        }

        let value = graph.get_value(&env.output()).unwrap();
        assert!(value <= 0.01, "value {} not near zero", value);
    }

    #[test]
    fn velocity_scales_output() {
        let mut graph = Graph::new(48_000.0);
        let env = graph.add_node(AdsrEnvelope::new(0.0, 0.0, 1.0, 0.01));

        let _ = graph
            .insert_value_input(env.attack(), 0.0)
            .expect("attack input available");
        let _ = graph
            .insert_value_input(env.decay(), 0.0)
            .expect("decay input available");
        let _ = graph
            .insert_value_input(env.sustain(), 1.0)
            .expect("sustain input available");
        let _ = graph
            .insert_value_input(env.release(), 0.01)
            .expect("release input available");

        graph.queue_event(env.gate(), 0, EventPayload::scalar(0.5));
        for _ in 0..100 {
            graph.process().expect("graph processes");
        }

        let value = graph.get_value(&env.output()).unwrap();
        assert!(
            value >= 0.45 && value <= 0.55,
            "value {} not scaled by velocity",
            value
        );
    }
}
