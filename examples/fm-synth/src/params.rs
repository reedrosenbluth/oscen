use nih_plug::prelude::*;
use nih_plug_slint::SlintState;
use std::sync::Arc;

/// Parameters for Operator 3 (top modulator)
#[derive(Params)]
pub struct Op3Params {
    #[id = "ratio"]
    pub ratio: FloatParam,

    #[id = "level"]
    pub level: FloatParam,

    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,
}

impl Default for Op3Params {
    fn default() -> Self {
        Self {
            ratio: FloatParam::new(
                "OP3 Ratio",
                3.0,
                FloatRange::Linear { min: 0.5, max: 16.0 },
            )
            .with_step_size(0.5)
            .with_smoother(SmoothingStyle::Linear(50.0)),

            level: FloatParam::new(
                "OP3 Level",
                0.5,
                FloatRange::Linear { min: 0.0, max: 2.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            feedback: FloatParam::new(
                "OP3 Feedback",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            attack: FloatParam::new(
                "OP3 Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            decay: FloatParam::new(
                "OP3 Decay",
                0.1,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            sustain: FloatParam::new(
                "OP3 Sustain",
                0.7,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            release: FloatParam::new(
                "OP3 Release",
                0.3,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),
        }
    }
}

/// Parameters for Operator 2 (middle modulator)
#[derive(Params)]
pub struct Op2Params {
    #[id = "ratio"]
    pub ratio: FloatParam,

    #[id = "level"]
    pub level: FloatParam,

    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,
}

impl Default for Op2Params {
    fn default() -> Self {
        Self {
            ratio: FloatParam::new(
                "OP2 Ratio",
                2.0,
                FloatRange::Linear { min: 0.5, max: 16.0 },
            )
            .with_step_size(0.5)
            .with_smoother(SmoothingStyle::Linear(50.0)),

            level: FloatParam::new(
                "OP2 Level",
                0.5,
                FloatRange::Linear { min: 0.0, max: 2.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            feedback: FloatParam::new(
                "OP2 Feedback",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            attack: FloatParam::new(
                "OP2 Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            decay: FloatParam::new(
                "OP2 Decay",
                0.1,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            sustain: FloatParam::new(
                "OP2 Sustain",
                0.7,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            release: FloatParam::new(
                "OP2 Release",
                0.3,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),
        }
    }
}

/// Parameters for Operator 1 (carrier) - no ratio/level/feedback controls
#[derive(Params)]
pub struct Op1Params {
    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,
}

impl Default for Op1Params {
    fn default() -> Self {
        Self {
            attack: FloatParam::new(
                "OP1 Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            decay: FloatParam::new(
                "OP1 Decay",
                0.2,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            sustain: FloatParam::new(
                "OP1 Sustain",
                0.8,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            release: FloatParam::new(
                "OP1 Release",
                0.5,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),
        }
    }
}

/// Filter parameters
#[derive(Params)]
pub struct FilterParams {
    #[id = "cutoff"]
    pub cutoff: FloatParam,

    #[id = "resonance"]
    pub resonance: FloatParam,

    #[id = "env_amount"]
    pub env_amount: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: FloatParam::new(
                "Filter Cutoff",
                2000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            resonance: FloatParam::new(
                "Filter Resonance",
                0.707,
                FloatRange::Linear { min: 0.1, max: 10.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            env_amount: FloatParam::new(
                "Filter Env Amount",
                0.0,
                FloatRange::Linear {
                    min: -10000.0,
                    max: 10000.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" Hz"),

            attack: FloatParam::new(
                "Filter Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            decay: FloatParam::new(
                "Filter Decay",
                0.2,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),

            sustain: FloatParam::new(
                "Filter Sustain",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),

            release: FloatParam::new(
                "Filter Release",
                0.3,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" s"),
        }
    }
}

/// Main plugin parameters with nested groups
#[derive(Params)]
pub struct PivotParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<SlintState>,

    #[nested(id_prefix = "op3", group = "Operator 3")]
    pub op3: Op3Params,

    #[nested(id_prefix = "op2", group = "Operator 2")]
    pub op2: Op2Params,

    #[nested(id_prefix = "op1", group = "Operator 1")]
    pub op1: Op1Params,

    #[id = "route"]
    pub route: FloatParam,

    #[nested(id_prefix = "filter", group = "Filter")]
    pub filter: FilterParams,
}

impl Default for PivotParams {
    fn default() -> Self {
        Self {
            editor_state: SlintState::from_size(750, 400),
            op3: Op3Params::default(),
            op2: Op2Params::default(),
            op1: Op1Params::default(),
            route: FloatParam::new("Route", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(50.0)),
            filter: FilterParams::default(),
        }
    }
}
