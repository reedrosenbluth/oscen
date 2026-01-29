use nih_plug::prelude::*;
use nih_plug_slint::SlintState;
use std::sync::Arc;

/// Create a time parameter (attack, decay, release) with skewed range and seconds unit.
fn time_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min: 0.001,
            max: 2.0,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_smoother(SmoothingStyle::Linear(50.0))
    .with_unit(" s")
}

/// Create a level parameter (0.0 to max) with linear range.
fn level_param(name: &str, default: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max })
        .with_smoother(SmoothingStyle::Linear(50.0))
}

/// Create a ratio parameter (0.5 to 16.0) with 0.5 step size.
fn ratio_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.5, max: 16.0 })
        .with_step_size(0.5)
        .with_smoother(SmoothingStyle::Linear(50.0))
}

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
            ratio: ratio_param("OP3 Ratio", 3.0),
            level: level_param("OP3 Level", 0.5, 2.0),
            feedback: level_param("OP3 Feedback", 0.0, 1.0),
            attack: time_param("OP3 Attack", 0.01),
            decay: time_param("OP3 Decay", 0.1),
            sustain: level_param("OP3 Sustain", 0.7, 1.0),
            release: time_param("OP3 Release", 0.3),
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
            ratio: ratio_param("OP2 Ratio", 2.0),
            level: level_param("OP2 Level", 0.5, 2.0),
            feedback: level_param("OP2 Feedback", 0.0, 1.0),
            attack: time_param("OP2 Attack", 0.01),
            decay: time_param("OP2 Decay", 0.1),
            sustain: level_param("OP2 Sustain", 0.7, 1.0),
            release: time_param("OP2 Release", 0.3),
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
            attack: time_param("OP1 Attack", 0.01),
            decay: time_param("OP1 Decay", 0.2),
            sustain: level_param("OP1 Sustain", 0.8, 1.0),
            release: time_param("OP1 Release", 0.5),
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
            attack: time_param("Filter Attack", 0.01),
            decay: time_param("Filter Decay", 0.2),
            sustain: level_param("Filter Sustain", 0.5, 1.0),
            release: time_param("Filter Release", 0.3),
        }
    }
}

/// Main plugin parameters with nested groups
#[derive(Params)]
pub struct FMParams {
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

impl Default for FMParams {
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
