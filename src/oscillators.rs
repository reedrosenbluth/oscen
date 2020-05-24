use super::graph::*;
use math::round::floor;
use std::any::Any;
use std::ops::{Index, IndexMut};

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

impl Index<&str> for SineOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SineOsc only does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SineOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SineOsc only does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SineOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        assert!(n < graph.nodes.len());
        if let Some(v) = graph.nodes[n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub struct SawOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SawOsc {
    pub fn new(hz: In) -> Self {
        SawOsc {
            hz,
            amplitude: fix(1.0),
            phase: fix(0.0),
        }
    }

    pub fn wrapped(hz: In) -> ArcMutex<Self> {
        arc(SawOsc::new(hz))
    }
}

impl Signal for SawOsc {
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
        let t = phase - 0.5;
        let s = -t - floor(0.5 - t, 0);
        if s < -0.499 {
            0.0
        } else {
            amplitude * 2.0 * s
        }
    }
}

impl Index<&str> for SawOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SawOsc only does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SawOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SawOsc only does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SawOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        assert!(n < graph.nodes.len());
        if let Some(v) = graph.nodes[n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
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

impl Index<&str> for SquareOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("SquareOsc only does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SquareOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SquareOsc only does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SquareOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        assert!(n < graph.nodes.len());
        if let Some(v) = graph.nodes[n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
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

impl Index<&str> for Osc01 {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "phase" => &self.phase,
            _ => panic!("Osc01 only does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Osc01 {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "phase" => &mut self.phase,
            _ => panic!("Osc01 only does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for Osc01 {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        assert!(n < graph.nodes.len());
        if let Some(v) = graph.nodes[n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = fix(value);
        }
    }
}

pub fn set_hz(graph: &Graph, n: Tag, hz: Real) {
    assert!(n < graph.nodes.len());
    SineOsc::set(graph, n, "hz", hz);
    SawOsc::set(graph, n, "hz", hz);
    SquareOsc::set(graph, n, "hz", hz);
    Osc01::set(graph, n, "hz", hz);
}