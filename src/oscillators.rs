use super::graph::*;
use math::round::floor;
use rand::distributions::Uniform;
use rand::prelude::*;
use std::any::Any;
use std::ops::{Index, IndexMut};

/// The most recent note received from the midi source.
pub struct MidiPitch {
    pub hz: In,
}

impl MidiPitch {
    pub fn new() -> Self {
        MidiPitch {
            hz: fix(0.0),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }

    pub fn set_hz(&mut self, hz: Real) {
        self.hz = fix(hz);
    }
}

impl Signal for MidiPitch {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        In::val(graph, self.hz)
    }
}

impl Index<&str> for MidiPitch {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            _ => panic!("MidiPitch does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for MidiPitch {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            _ => panic!("MidiPitch does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for MidiPitch {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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

/// A basic sine oscillator.
#[derive(Copy, Clone)]
pub struct SineOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SineOsc {
    pub fn new() -> Self {
        Self {
            hz: fix(0.0),
            amplitude: fix(1.0),
            phase: fix(0.0),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {hz, amplitude: fix(1.0), phase: fix(0.0)}
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
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
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
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
            _ => panic!("SineOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SineOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SineOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SineOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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

/// Saw wave oscillator.
pub struct SawOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl SawOsc {
    pub fn new() -> Self {
        Self {
            hz: fix(0.0),
            amplitude: fix(1.0),
            phase: fix(0.0),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {hz, amplitude: fix(1.0), phase: fix(0.0)}
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
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
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
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
            _ => panic!("SawOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SawOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SawOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SawOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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

/// Triangle wave oscillator.
pub struct TriangleOsc {
    pub hz: In,
    pub amplitude: In,
    pub phase: In,
}

impl TriangleOsc {
    pub fn new() -> Self {
        Self {
            hz: fix(0.0),
            amplitude: fix(1.0),
            phase: fix(0.0),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {hz, amplitude: fix(1.0), phase: fix(0.0)}
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for TriangleOsc {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let amplitude = In::val(graph, self.amplitude);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
        };
        let t = phase - 0.75;
        let saw_amp = 2. * (-t - floor(0.5 - t, 0));
        (2. * saw_amp.abs() - amplitude) * amplitude
    }
}

impl Index<&str> for TriangleOsc {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "hz" => &self.hz,
            "amp" => &self.amplitude,
            "phase" => &self.phase,
            _ => panic!("TriangleOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for TriangleOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("TriangleOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for TriangleOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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
    pub fn new() -> Self {
        Self {
            hz: fix(0.0),
            amplitude: fix(1.0),
            phase: fix(0.0),
            duty_cycle: fix(0.5),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {hz, amplitude: fix(1.0), phase: fix(0.0), duty_cycle: fix(0.5)}
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
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
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
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
            _ => panic!("SquareOsc does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for SquareOsc {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "amp" => &mut self.amplitude,
            "phase" => &mut self.phase,
            _ => panic!("SquareOsc does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for SquareOsc {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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
pub struct WhiteNoise {
    pub amplitude: In,
    dist: Uniform<Real>,
}

impl WhiteNoise {
    pub fn new() -> Self {
        Self {
            amplitude: fix(1.0),
            dist: Uniform::new_inclusive(-1.0, 1.0),
        }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for WhiteNoise {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, _sample_rate: Real) -> Real {
        let mut rng = rand::thread_rng();
        let amplitude = In::val(graph, self.amplitude);
        self.dist.sample(&mut rng) * amplitude
    }
}

impl Index<&str> for WhiteNoise {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "amp" => &self.amplitude,
            _ => panic!("WhiteNoise does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for WhiteNoise {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "amp" => &mut self.amplitude,
            _ => panic!("WhiteNoise does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for WhiteNoise {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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
    pub fn new() -> Self {
        Self {
            hz: fix(0.0),
            phase: fix(0.0),
        }
    }

    pub fn with_hz(hz: In) -> Self {
        Self {hz, phase: fix(0.0)}
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for Osc01 {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        let hz = In::val(graph, self.hz);
        let phase = In::val(graph, self.phase);
        match &self.phase {
            In::Fix(p) => {
                let mut ph = *p + hz / sample_rate;
                ph %= sample_rate;
                self.phase = In::Fix(ph);
            }
            In::Cv(_) => {}
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
            _ => panic!("Osc01 does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Osc01 {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "hz" => &mut self.hz,
            "phase" => &mut self.phase,
            _ => panic!("Osc01 does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for Osc01 {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
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

/// "pattern match" node on each oscillator type and set hz
pub fn set_hz(graph: &Graph, n: Tag, hz: Real) {
    SineOsc::set(graph, n, "hz", hz);
    SawOsc::set(graph, n, "hz", hz);
    TriangleOsc::set(graph, n, "hz", hz);
    SquareOsc::set(graph, n, "hz", hz);
    Osc01::set(graph, n, "hz", hz);
}
