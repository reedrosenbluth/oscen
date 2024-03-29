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

    pub fn on(&self, rack: &mut Rack) {
        self.adsr.on(rack);
    }

    pub fn off(&self, rack: &mut Rack) {
        self.adsr.off(rack);
    }

    pub fn set_adsr_attack(&self, rack: &mut Rack, value: Control) {
        self.adsr.set_attack(rack, value);
    }

    pub fn set_adsr_decay(&self, rack: &mut Rack, value: Control) {
        self.adsr.set_decay(rack, value);
    }

    pub fn set_adsr_sustain(&self, rack: &mut Rack, value: Control) {
        self.adsr.set_sustain(rack, value);
    }

    pub fn set_adsr_release(&self, rack: &mut Rack, value: Control) {
        self.adsr.set_release(rack, value);
    }
}

impl Signal for WaveGuide {
    tag!();

    fn signal(&self, rack: &mut Rack, _sample_rate: f32) {
        rack.outputs[(self.tag, 0)] = rack.outputs[(self.mixer.tag(), 0)];
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
    pub fn rack(&self, rack: &mut Rack) -> Arc<WaveGuide> {
        let adsr = AdsrBuilder::exp_20()
            .attack(0.001)
            .decay(0.0)
            .sustain(0.0)
            .release(0.001)
            .rack(rack);
        let exciter = ProductBuilder::new(vec![self.burst, adsr.tag()]).rack(rack);
        let mixer = MixerBuilder::new(vec![0.into(), 0.into()]).rack(rack);
        let delay = DelayBuilder::new(mixer.tag(), self.hz_inv).rack(rack);
        let lpf = LpfBuilder::new(delay.tag()).cut_off(self.cutoff).rack(rack);
        let lpf_vca = VcaBuilder::new(lpf.tag()).level(self.decay).rack(rack);
        rack.controls[(mixer.tag(), 0)] = Control::I(exciter.tag().into());
        rack.controls[(mixer.tag(), 1)] = Control::I(lpf_vca.tag().into());
        let n = rack.num_modules();
        rack.controls[(n, 0)] = self.hz_inv;
        rack.controls[(n, 1)] = self.cutoff;
        rack.controls[(n, 2)] = self.decay;
        let wg = Arc::new(WaveGuide::new(n, self.burst, adsr, mixer));
        rack.push(wg.clone());
        wg
    }
}
