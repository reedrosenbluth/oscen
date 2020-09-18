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
    adsr: Arc<Adsr>,
}

impl WaveGuide {
    pub fn new<T: Into<Tag>>(tag: T, burst: Tag, adsr: Arc<Adsr>) -> Self {
        Self { tag: tag.into(), burst, adsr }
    }
    props!(hz, set_hz, 0);
    props!(cutoff, set_cutoff, 1);
    props!(decay, set_decay, 2);
    props!(delay, set_delay, 3);

    pub fn on(&self, controls: &mut Controls, state: &mut State) {
        self.adsr.on(controls, state);
    }

    pub fn off(&self, controls: &mut Controls) {
        self.adsr.off(controls);
    }

    pub fn set_adsr_attack(&self, controls: &mut Controls, value: Control) {
        self.adsr.set_attack(controls, value);
    }

    pub fn set_adsr_decay(&self, controls: &mut Controls, value: Control) {
        self.adsr.set_decay(controls, value);
    }

    pub fn set_adsr_sustain(&self, controls: &mut Controls, value: Control) {
        self.adsr.set_sustain(controls, value);
    }

    pub fn set_adsr_release(&self, controls: &mut Controls, value: Control) {
        self.adsr.set_release(controls, value);
    }
}

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
    pub fn new(burst: Tag) -> Self {
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
    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls, buffers: &mut Buffers) -> Arc<WaveGuide> {
        let adsr = AdsrBuilder::exp_20()
            .attack(0.001)
            .decay(0.0)
            .sustain(0.0)
            .release(0.001)
            .rack(rack, controls);
        let exciter = ProductBuilder::new(vec![self.burst, adsr.tag()]).rack(rack, controls);
        let mixer = MixerBuilder::new(vec![0.into(), 0.into()]).rack(rack, controls);
        let delay = DelayBuilder::new(mixer.tag(), self.delay).rack(rack, controls, buffers);
        let lpf = LpfBuilder::new(delay.tag()).cut_off(self.cutoff).rack(rack, controls);
        let lpf_vca = VcaBuilder::new(lpf.tag()).level(self.decay).rack(rack, controls);
        controls[(mixer.tag(), 0)] = Control::I(exciter.tag().into());
        controls[(mixer.tag(), 1)] = Control::I(lpf_vca.tag().into());
        let n = rack.num_modules();
        let wg = Arc::new(WaveGuide::new(n, self.burst, adsr));
        rack.push(wg.clone());
        wg
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
