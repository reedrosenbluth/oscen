use super::{
    envelopes::Adsr,
    filters::Lpf,
    operators::{Delay, Mixer, Product},
    signal::{mk_tag, ArcMutex, Builder, In, Link, Rack, Real, Signal, Tag},
};
use crate::{as_any_mut, std_signal};
use std::any::Any;

#[derive(Clone)]
pub struct WaveGuide {
    tag: Tag,
    burst: Tag,
    hz: In,
    cutoff_freq: In,
    wet_decay: In,
    input: ArcMutex<Link>,
    gate: ArcMutex<Adsr>,
    lpf: ArcMutex<Lpf>,
    delay: ArcMutex<Delay>,
    mixer: ArcMutex<Mixer>,
    rack: Rack,
}

impl WaveGuide {
    pub fn new(burst: Tag) -> Self {
        let mut rack = Rack::new(vec![]);

        let input = Link::new().wrap();
        rack.append(input.clone());

        // Adsr
        let gate = Adsr::new(0.2, 0.2, 0.2)
            .attack(0.001)
            .decay(0)
            .sustain(0)
            .release(0.001)
            .wrap();
        rack.append(gate.clone());

        // Exciter: gated noise
        let exciter = Product::new(vec![input.tag(), gate.tag()]).wrap();
        rack.append(exciter.clone());

        // Feedback loop
        let mut mixer = Mixer::new(vec![]).build();
        let delay = Delay::new(mixer.tag(), (0.02).into()).wrap();

        let cutoff_freq = 2000;
        let lpf = Lpf::new(delay.tag()).cutoff_freq(cutoff_freq).wrap();

        let wet_decay = 0.95;
        let mixer = mixer
            .waves(vec![exciter.tag(), lpf.tag()])
            .levels(vec![1.0, wet_decay])
            .wrap();

        rack.append(lpf.clone());
        rack.append(delay.clone());
        rack.append(mixer.clone());

        WaveGuide {
            tag: mk_tag(),
            burst,
            hz: 440.into(),
            cutoff_freq: cutoff_freq.into(),
            wet_decay: wet_decay.into(),
            input,
            gate,
            lpf,
            delay,
            mixer,
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

    pub fn attack<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.gate.lock().unwrap().attack(arg);
        self
    }

    pub fn decay<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.gate.lock().unwrap().decay(arg);
        self
    }

    pub fn sustain<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.gate.lock().unwrap().sustain(arg);
        self
    }

    pub fn release<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.gate.lock().unwrap().release(arg);
        self
    }

    pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.lpf.lock().unwrap().cutoff_freq(arg);
        self
    }

    pub fn wet_decay<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.mixer.lock().unwrap().level_nth(1, arg.into());
        self
    }
}

impl Builder for WaveGuide {}

impl Signal for WaveGuide {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let input = rack.output(self.burst);
        self.input.lock().unwrap().value(input);
        let dt = 1.0 / f64::max(1.0, In::val(&rack, self.hz));
        self.delay.lock().unwrap().delay_time(dt);
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
