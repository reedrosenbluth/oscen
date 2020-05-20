use super::graph::*;
use super::operators::*;
use super::oscillators::*;
use std::any::Any;

/// Interpolate between the three oscillators depending on the value of `knob`.
/// If `knob` is less thanb 1/2 then interpolate between square wave and sin wave,
/// otherwise interpolate between sine wave and saw wave.

pub struct WaveShaper(pub Lerp3);

impl WaveShaper {
    pub fn new(hz: In, knob: In) -> Self {
        let square = SquareOsc::new(hz);
        let sine = SineOsc::new(hz);
        let sine2 = sine.clone();
        let saw = SawOsc::new(hz);
        let lerp1 = Lerp::new(square, sine);
        let lerp2 = Lerp::new(sine2, saw);
        let lerp3 = Lerp3::new(lerp1, lerp2, knob);
        WaveShaper(lerp3)
    }

    pub fn wrapped(hz: In, knob: In) -> ArcMutex<Self> {
        arc(Self::new(hz, knob))
    }

    pub fn set_alphas(&mut self, graph: &Graph) {
        self.0.set_alphas(graph)
    }
}

impl Signal for WaveShaper {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        self.0.signal(graph, sample_rate)
    }
}