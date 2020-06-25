use super::{envelopes::*, filters::*, operators::*, signal::*};
use crate::{as_any_mut, std_signal};
use std::any::Any;

#[derive(Clone)]
pub struct WaveGuide {
    tag: Tag,
    burst: Tag,
    hz: In,
    input: ArcMutex<Link>,
    gate: ArcMutex<Adsr>,
    delay: ArcMutex<Delay>,
    rack: Rack,
}

impl WaveGuide {
    pub fn new(burst: Tag) -> Self {
        let mut rack = Rack::new(vec![]);

        let input = Link::new().wrap();
        rack.append(input.clone());

        // Adsr
        let adsr = Adsr::new(0.2, 0.2, 0.2)
            .attack(0.001)
            .decay(0)
            .sustain(0)
            .release(0.001)
            .wrap();
        rack.append(adsr.clone());

        // Exciter: gated noise
        let exciter = Product::new(vec![input.tag(), adsr.tag()]).wrap();
        rack.append(exciter.clone());

        // Feedback loop
        let mut mixer = Mixer::new(vec![]).build();
        let delay = Delay::new(mixer.tag(), (0.02).into()).wrap();
        let lpf = Lpf::new(delay.tag()).cutoff_freq(2000).wrap();
        let mixer = mixer
            .waves(vec![exciter.tag(), lpf.tag()])
            .levels(vec![1.0, 0.95])
            .wrap();

        rack.append(lpf);
        rack.append(delay.clone());
        rack.append(mixer);

        WaveGuide {
            tag: mk_tag(),
            burst,
            hz: 440.into(),
            input,
            gate: adsr,
            delay,
            rack,
        }
    }

    pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.hz = arg.into();
        self
    }

    pub fn on(&mut self) {
        self.gate.lock().unwrap().on();
    }

    pub fn off(&mut self) {
        self.gate.lock().unwrap().off();
    }
}

impl Builder for WaveGuide {}

impl Signal for WaveGuide {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let input = rack.output(self.burst);
        self.input.lock().unwrap().value = input.into();
        let dt = 1.0 / f64::max(1.0, In::val(&rack, self.hz));
        self.delay.lock().unwrap().delay_time = dt.into();
        self.rack.signal(sample_rate)
    }
}

pub fn on(rack: &Rack, n: Tag) {
    if let Some(v) = rack.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<WaveGuide>()
    {
        v.on();
    }
}

pub fn off(rack: &Rack, n: Tag) {
    if let Some(v) = rack.nodes[&n]
        .module
        .lock()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<WaveGuide>()
    {
        v.off();
    }
}
