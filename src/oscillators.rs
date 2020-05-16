
use super::graph::*;
use math::round::floor;
use std::any::Any;

/// A basic sine oscillator.
#[derive(Clone)]
pub struct SineOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SineOsc {
    pub fn new(hz: In) -> Self {
        SineOsc {
            hz,
            amplitude: fix(1.0),
            phase: fix(0.0),
        }
    }

    pub fn wrapped(hz: In) -> ArcMutex<Self> {
        arc(SineOsc::new(hz))
    }
}

impl Signal for SineOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        self.phase = match &self.phase {
            In::Fixed(p) => {
                let mut ph = p + hz / sample_rate;
                ph %= sample_rate;
                In::Fixed(ph)
            }
            In::Var(x) => In::Var(*x),
        };
        amplitude * (TAU * phase).sin()
    }
}
/// An oscillator used to modulate parameters that take values between 0 and 1,
/// based on a sinusoid.
pub struct Osc01 {
    pub hz: In,
    pub phase: In,
}

impl Osc01 {
    pub fn new(hz: In) -> Self {
        Osc01 {
            hz,
            phase: fix(0.0),
        }
    }

    pub fn wrapped(hz: In) -> ArcMutex<Self> {
        arc(Osc01::new(hz))
    }
}

impl Signal for Osc01 {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let phase = In::val(graph, self.phase);
        self.phase = match &self.phase {
            In::Fixed(p) => {
                let mut ph = p + hz / sample_rate;
                ph %= sample_rate;
                fix(ph)
            }
            In::Var(x) => In::Var(*x),
        };
        0.5 * ((TAU * phase).sin() + 1.0)
    }
}

/// Square wave oscillator with a `duty_cycle` that takes values in (0, 1).
#[derive(Clone)]
pub struct SquareOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
    pub duty_cycle: In,
}

impl SquareOsc {
    pub fn new(hz: In) -> Self {
        SquareOsc {
            hz,
            amplitude: fix(1.0),
            phase: fix(0.0),
            duty_cycle: fix(0.5),
        }
    }

    pub fn wrapped(hz: In) -> ArcMutex<Self> {
        arc(SquareOsc::new(hz))
    }
}

impl Signal for SquareOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        self.phase = match &self.phase {
            In::Fixed(p) => {
                let mut ph = p + hz / sample_rate;
                ph %= sample_rate;
                fix(ph)
            }
            In::Var(x) => In::Var(*x),
        };

        let duty_cycle = In::val(graph, self.duty_cycle);
        let t = phase - floor(phase, 0);
        if t < 0.001 {
            0.0
        } else if t <= duty_cycle {
            amplitude
        } else {
            -amplitude
        }
    }
}
