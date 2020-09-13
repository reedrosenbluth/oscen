use crate::{build, props, tag};
use crate::{
    envelopes::*,
    filters::LpfBuilder,
    operators::*,
    rack::*,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct WaveGuide {
    tag: Tag,
    burst: Tag,
    // hz: In,
    // cutoff_freq: In,
    // wet_decay: In,
    // input: ArcMutex<Link>,
    // envelope: ArcMutex<Adsr>,
    // lpf: ArcMutex<Lpf>,
    // delay: ArcMutex<Delay>,
    // mixer: ArcMutex<Mixer>,
}

impl WaveGuide {
    pub fn new(tag: Tag, burst: Tag) -> Self {
        Self { tag, burst }
    }
    props!(hz, set_hz, 0);
    props!(cutoff, set_cutoff, 1);
    props!(decay, set_decay, 2);
    props!(delay, set_delay, 3);
}

// pub fn new(tag: Tag, burst: Tag) -> Self {

// let input = Link::new(&mut id).wrap();
// rack.append(input.clone());

// Adsr
//     let envelope = Adsr::new(&mut id, 0.2, 0.2, 0.2)
//         .attack(0.001)
//         .decay(0)
//         .sustain(0)
//         .release(0.001)
//         .wrap();
//     rack.append(envelope.clone());

//     // Exciter: gated noise
//     let exciter = Product::new(&mut id, vec![input.tag(), envelope.tag()]).wrap();
//     rack.append(exciter.clone());

//     // Feedback loopv
//     let mut mixer = Mixer::new(&mut id, vec![]).build();
//     let delay = Delay::new(&mut id, mixer.tag(), (0.02).into()).wrap();

//     let cutoff_freq = 2000;
//     let lpf = Lpf::new(&mut id, delay.tag())
//         .cutoff_freq(cutoff_freq)
//         .wrap();

//     let wet_decay = 0.95;
//     let mixer = mixer
//         .waves(vec![exciter.tag(), lpf.tag()])
//         .levels(vec![1.0, wet_decay])
//         .wrap();

//     rack.append(lpf.clone());
//     rack.append(delay.clone());
//     rack.append(mixer.clone());

//     WaveGuide {
//         tag: id_gen.id(),
//         burst,
//         hz: 440.into(),
//         cutoff_freq: cutoff_freq.into(),
//         wet_decay: wet_decay.into(),
//         input,
//         envelope,
//         lpf,
//         delay,
//         mixer,
//         rack,
//         out: 0.0,
//     }
// }

//     pub fn hz<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.hz = arg.into();
//         self
//     }

//     pub fn on(&mut self) {
//         self.envelope.lock().on();
//     }

//     pub fn off(&mut self) {
//         self.envelope.lock().off();
//     }

//     pub fn attack<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.envelope.lock().attack(arg);
//         self
//     }

//     pub fn decay<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.envelope.lock().decay(arg);
//         self
//     }

//     pub fn sustain<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.envelope.lock().sustain(arg);
//         self
//     }

//     pub fn release<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.envelope.lock().release(arg);
//         self
//     }

//     pub fn cutoff_freq<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.lpf.lock().cutoff_freq(arg);
//         self
//     }

//     pub fn wet_decay<T: Into<In>>(&mut self, arg: T) -> &mut Self {
//         self.mixer.lock().level_nth(1, arg.into());
//         self
//     }
// }

impl Signal for WaveGuide {
    tag!();

    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let input = outputs[(self.burst, 0)];
        let dt = 1.0 / f32::max(1.0, self.hz(controls, outputs));
    }
}

#[derive(Clone)]
pub struct WaveGuideBuilder {
    burst: Tag,
    hz: Control,
    cutoff: Control,
    decay: Control,
    delay: Control,
}

impl WaveGuideBuilder {
    pub fn new(burst: Tag, buffer: RingBuffer) -> Self {
        Self {
            burst,
            hz: 440.0.into(),
            cutoff: 2000.0.into(),
            decay: 0.95.into(),
            delay: 0.02.into(),
        }
    }
    build!(hz);
    build!(cutoff);
    build!(decay);
    build!(delay);
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls, buffers: &mut Buffers) {
        let adsr = AdsrBuilder::exp_20()
            .attack(0.001)
            .decay(0.0)
            .sustain(0.0)
            .release(0.001)
            .rack(rack, controls);
        let exciter = ProductBuilder::new(vec![self.burst, adsr.tag()]).rack(rack, controls);
        let mixer = MixerBuilder::new(vec![0.into(), 0.into()]).rack(rack, controls);
        // let delay = DelayBuilder::new(mixer.tag(), self.delay()).rack(rack, buffers);
        // let lpf = LpfBuilder::new(delay.tag()).cut_off(self.cutoff).rack(rack, controls);
        // let lpf_vca = VcaBuilder::new(lpf.tag()).level(self.decay).rack(rack, controls);
        controls[(mixer.tag(), 0)] = Control::I(exciter.tag().into());
        // controls[(mixer.tag(), 1)] = Control::I(lpf_vca.tag().into());
    }
}

// impl Signal for WaveGuide {
//     std_signal!();
//     fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
//         let input = rack.output(self.burst);
//         self.input.lock().value(input);
//         let dt = 1.0 / f64::max(1.0, In::val(&rack, self.hz));
//         self.delay.lock().delay_time(dt);
//         self.out = self.rack.signal(sample_rate);
//         self.out
//     }
// }
