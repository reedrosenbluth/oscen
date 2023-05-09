use crate::{build, props, tag};
use crate::{envelopes::*, filters::LpfBuilder, operators::*, rack::*};
use std::sync::Arc;

#[derive(Clone)]
pub struct WaveGuide {
    tag: Tag,
    _burst: Tag,
    adsr: Arc<Adsr>,
    mixer: Arc<Mixer>,
}

impl WaveGuide {
    pub fn new<T: Into<Tag>>(tag: T, burst: Tag, adsr: Arc<Adsr>, mixer: Arc<Mixer>) -> Self {
        Self {
            tag: tag.into(),
            _burst: burst,
            adsr,
            mixer,
        }
    }
    props!(hz_inv, set_hz_inv, 0);
    props!(cutoff, set_cutoff, 1);
    props!(decay, set_decay, 2);

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
        _controls: &Controls,
        _state: &mut State,
        outputs: &mut Outputs,
        _buffers: &mut Buffers,
        _sample_rate: f32,
    ) {
        outputs[(self.tag, 0)] = outputs[(self.mixer.tag(), 0)];
    }
}

#[derive(Clone)]
pub struct WaveGuideBuilder {
    burst: Tag,
    hz_inv: Control,
    cutoff: Control,
    decay: Control,
}

impl WaveGuideBuilder {
    pub fn new(burst: Tag) -> Self {
        Self {
            burst,
            hz_inv: (1.0 / 440.0).into(),
            cutoff: 2000.0.into(),
            decay: 0.95.into(),
        }
    }
    build!(hz_inv);
    build!(cutoff);
    build!(decay);
    pub fn rack(
        &self,
        rack: &mut Rack,
        controls: &mut Controls,
        buffers: &mut Buffers,
    ) -> Arc<WaveGuide> {
        let adsr = AdsrBuilder::exp_20()
            .attack(0.001)
            .decay(0.0)
            .sustain(0.0)
            .release(0.001)
            .rack(rack, controls);
        let exciter = ProductBuilder::new(vec![self.burst, adsr.tag()]).rack(rack, controls);
        let mixer = MixerBuilder::new(vec![0.into(), 0.into()]).rack(rack, controls);
        let delay = DelayBuilder::new(mixer.tag(), self.hz_inv).rack(rack, controls, buffers);
        let lpf = LpfBuilder::new(delay.tag())
            .cut_off(self.cutoff)
            .rack(rack, controls);
        let lpf_vca = VcaBuilder::new(lpf.tag())
            .level(self.decay)
            .rack(rack, controls);
        controls[(mixer.tag(), 0)] = Control::I(exciter.tag().into());
        controls[(mixer.tag(), 1)] = Control::I(lpf_vca.tag().into());
        let n = rack.num_modules();
        controls[(n, 0)] = self.hz_inv;
        controls[(n, 1)] = self.cutoff;
        controls[(n, 2)] = self.decay;
        let wg = Arc::new(WaveGuide::new(n, self.burst, adsr, mixer));
        rack.push(wg.clone());
        wg
    }
}
