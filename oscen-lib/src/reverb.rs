use crate::filters::*;
use crate::operators::*;
use crate::rack::*;
use crate::{build, props, tag};
use std::sync::Arc;

const FIXED_GAIN: f32 = 0.015;

const SCALE_WET: f32 = 3.0;
const SCALE_DAMPENING: f32 = 0.4;

const SCALE_ROOM: f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;

const STEREO_SPREAD: usize = 23;

const COMB_TUNING_L1: usize = 1116;
const COMB_TUNING_R1: usize = 1116 + STEREO_SPREAD;
const COMB_TUNING_L2: usize = 1188;
const COMB_TUNING_R2: usize = 1188 + STEREO_SPREAD;
const COMB_TUNING_L3: usize = 1277;
const COMB_TUNING_R3: usize = 1277 + STEREO_SPREAD;
const COMB_TUNING_L4: usize = 1356;
const COMB_TUNING_R4: usize = 1356 + STEREO_SPREAD;
const COMB_TUNING_L5: usize = 1422;
const COMB_TUNING_R5: usize = 1422 + STEREO_SPREAD;
const COMB_TUNING_L6: usize = 1491;
const COMB_TUNING_R6: usize = 1491 + STEREO_SPREAD;
const COMB_TUNING_L7: usize = 1557;
const COMB_TUNING_R7: usize = 1557 + STEREO_SPREAD;
const COMB_TUNING_L8: usize = 1617;
const COMB_TUNING_R8: usize = 1617 + STEREO_SPREAD;

const ALLPASS_TUNING_L1: usize = 556;
const ALLPASS_TUNING_R1: usize = 556 + STEREO_SPREAD;
const ALLPASS_TUNING_L2: usize = 441;
const ALLPASS_TUNING_R2: usize = 441 + STEREO_SPREAD;
const ALLPASS_TUNING_L3: usize = 341;
const ALLPASS_TUNING_R3: usize = 341 + STEREO_SPREAD;
const ALLPASS_TUNING_L4: usize = 225;
const ALLPASS_TUNING_R4: usize = 225 + STEREO_SPREAD;

#[derive(Clone)]
pub struct Freeverb {
    tag: Tag,
    wave_l: Tag,
    wave_r: Tag,
    left: Arc<AllPass>,
    right: Arc<AllPass>,
}

impl Freeverb {
    pub fn new<T: Into<Tag>>(
        tag: T,
        wave_l: Tag,
        wave_r: Tag,
        left: Arc<AllPass>,
        right: Arc<AllPass>,
    ) -> Self {
        Self {
            tag: tag.into(),
            wave_l,
            wave_r,
            left,
            right,
        }
    }

    props!(wet_gain_l, set_wet_gain_l, 0);
    props!(wet_gain_r, set_wet_gain_r, 1);
    props!(wet, set_wet, 2);
    props!(width, set_width, 3);
    props!(dry, set_dry, 4);
    props!(input_gain, set_input_gain, 5);
    props!(dampening, set_dampening, 6);
    props!(room_size, set_room_size, 7);

    pub fn frozen(&self, controls: &Controls, outputs: &Outputs) -> bool {
        let inp = controls[(self.tag, 8)];
        outputs.boolean(inp).unwrap()
    }

    pub fn set_frozen(&self, controls: &mut Controls, value: Control) {
        controls[(self.tag, 8)] = value;
    }
}
// pub fn tick(&mut self, input: (f64, f64)) -> (f64, f64) {
//     let input_mixed = (input.0 + input.1) * FIXED_GAIN * self.input_gain;
//
//     let mut out = (0.0, 0.0);
//
//     for combs in self.combs.iter_mut() {
//         out.0 += combs.0.tick(input_mixed);
//         out.1 += combs.1.tick(input_mixed);
//     }
//
//     for allpasses in self.allpasses.iter_mut() {
//         out.0 = allpasses.0.tick(out.0);
//         out.1 = allpasses.1.tick(out.1);
//     }
//
//     (
//         out.0 * self.wet_gains.0 + out.1 * self.wet_gains.1 + input.0 * self.dry,
//         out.1 * self.wet_gains.0 + out.0 * self.wet_gains.1 + input.1 * self.dry,
//     )
// }

impl Signal for Freeverb {
    tag!();

    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    ) {
        let input_l = outputs[(self.wave_l, 0)];
        let input_r = outputs[(self.wave_r, 0)];
        let mixed_input = (outputs[(self.wave_l, 0)] + outputs[(self.wave_r, 0)])
            * FIXED_GAIN
            * self.input_gain(controls, outputs);
        outputs[(self.tag, 0)] =
            self.wet_gain_l(controls, outputs) * outputs[(self.right.tag(), 0)];
        outputs[(self.tag, 1)] =
            self.wet_gain_r(controls, outputs) * outputs[(self.right.tag(), 0)];
    }
}

out.0 * self.wet_gains.0 + out.1 * self.wet_gains.1 + input.0 * self.dry,
out.1 * self.wet_gains.0 + out.0 * self.wet_gains.1 + input.1 * self.dry,
//     std_signal!();
//     fn signal(&mut self, rack: &Rack, sample_rate: f32) -> f32 {
//         let inp = rack.output(self.wave);
//         self.input.lock().value(inp);
//         let out = self.rack.signal(sample_rate);
//         self.out = out * self.wet_gain + inp * self.dry;
//         self.out
//     }
// }

pub struct FreeverbBuilder {
    wave_l: Tag,
    wave_r: Tag,
    wet_gain_l: Control,
    wet_gain_r: Control,
    wet: Control,
    width: Control,
    dry: Control,
    input_gain: Control,
    dampening: Control,
    room_size: Control,
    frozen: Control,
}

impl FreeverbBuilder {
    pub fn new(wave_l: Tag, wave_r: Tag) -> Self {
        Self {
            wave_l,
            wave_r,
            wet_gain_l: 0.0.into(),
            wet_gain_r: 0.0.into(),
            wet: 0.0.into(),
            width: 0.0.into(),
            dry: 0.0.into(),
            input_gain: 0.0.into(),
            dampening: 0.0.into(),
            room_size: 0.0.into(),
            frozen: false.into(),
        }
    }
    build!(wet_gain_l);
    build!(wet_gain_r);
    build!(wet);
    build!(width);
    build!(dry);
    build!(input_gain);
    build!(dampening);
    build!(room_size);
    build!(frozen);

    pub fn rack(
        &self,
        rack: &mut Rack,
        controls: &mut Controls,
        buffers: &mut Buffers,
    ) -> Arc<Freeverb> {
        let n = rack.num_modules();
        controls[(n, 0)] = self.wet_gain_l;
        controls[(n, 1)] = self.wet_gain_r;
        controls[(n, 2)] = self.wet;
        controls[(n, 3)] = self.width;
        controls[(n, 4)] = self.dry;
        controls[(n, 5)] = self.input_gain;
        controls[(n, 6)] = self.dampening;
        controls[(n, 7)] = self.room_size;
        controls[(n, 8)] = self.frozen;

        let input = (outputs[(self.wave_l, 0)] + outputs[(self.wave_r, 0)])
            * FIXED_GAIN
            * self.input_gain(controls, outputs);
        let comb1_l = CombBuilder::new(self.wave_l, COMB_TUNING_L1).rack(rack, controls, buffers);
        let comb1_r = CombBuilder::new(self.wave_r, COMB_TUNING_R1).rack(rack, controls, buffers);
        let comb2_l = CombBuilder::new(self.wave_l, COMB_TUNING_L2).rack(rack, controls, buffers);
        let comb2_r = CombBuilder::new(self.wave_r, COMB_TUNING_R2).rack(rack, controls, buffers);
        let comb3_l = CombBuilder::new(self.wave_l, COMB_TUNING_L3).rack(rack, controls, buffers);
        let comb3_r = CombBuilder::new(self.wave_r, COMB_TUNING_R3).rack(rack, controls, buffers);
        let comb4_l = CombBuilder::new(self.wave_l, COMB_TUNING_L4).rack(rack, controls, buffers);
        let comb4_r = CombBuilder::new(self.wave_r, COMB_TUNING_R4).rack(rack, controls, buffers);
        let comb5_l = CombBuilder::new(self.wave_l, COMB_TUNING_L5).rack(rack, controls, buffers);
        let comb5_r = CombBuilder::new(self.wave_r, COMB_TUNING_R5).rack(rack, controls, buffers);
        let comb6_l = CombBuilder::new(self.wave_l, COMB_TUNING_L6).rack(rack, controls, buffers);
        let comb6_r = CombBuilder::new(self.wave_r, COMB_TUNING_R6).rack(rack, controls, buffers);
        let comb7_l = CombBuilder::new(self.wave_l, COMB_TUNING_L7).rack(rack, controls, buffers);
        let comb7_r = CombBuilder::new(self.wave_r, COMB_TUNING_R7).rack(rack, controls, buffers);
        let comb8_l = CombBuilder::new(self.wave_l, COMB_TUNING_L8).rack(rack, controls, buffers);
        let comb8_r = CombBuilder::new(self.wave_r, COMB_TUNING_R8).rack(rack, controls, buffers);
        let combs_l = MixerBuilder::new(vec![
            comb1_l.tag(),
            comb2_l.tag(),
            comb3_l.tag(),
            comb4_l.tag(),
            comb5_l.tag(),
            comb6_l.tag(),
            comb7_l.tag(),
            comb8_l.tag(),
        ])
        .rack(rack, controls);
        let combs_r = MixerBuilder::new(vec![
            comb1_r.tag(),
            comb2_r.tag(),
            comb3_r.tag(),
            comb4_r.tag(),
            comb5_r.tag(),
            comb6_r.tag(),
            comb7_r.tag(),
            comb8_r.tag(),
        ])
        .rack(rack, controls);
        let all1_l = AllPassBuilder::new(combs_l.tag(), ALLPASS_TUNING_L1).rack(rack, buffers);
        let all1_r = AllPassBuilder::new(combs_r.tag(), ALLPASS_TUNING_R1).rack(rack, buffers);
        let all2_l = AllPassBuilder::new(all1_l.tag(), ALLPASS_TUNING_L2).rack(rack, buffers);
        let all2_r = AllPassBuilder::new(all1_r.tag(), ALLPASS_TUNING_R2).rack(rack, buffers);
        let all3_l = AllPassBuilder::new(all2_l.tag(), ALLPASS_TUNING_L3).rack(rack, buffers);
        let all3_r = AllPassBuilder::new(all2_r.tag(), ALLPASS_TUNING_R3).rack(rack, buffers);
        let all4_l = AllPassBuilder::new(all3_l.tag(), ALLPASS_TUNING_L4).rack(rack, buffers);
        let all4_r = AllPassBuilder::new(all3_r.tag(), ALLPASS_TUNING_R4).rack(rack, buffers);
        let n = rack.num_modules();
        let fv = Arc::new(Freeverb::new(n, self.wave_l, self.wave_r, all4_l, all4_r));
        rack.push(fv.clone());
        fv
    }
}

// impl Freeverb {
// pub fn wave(&mut self, arg: Tag) -> &mut Self {
//     self.wave = arg;
//     self
// }

// pub fn dampening(&mut self, value: f32) -> &mut Self {
//     self.dampening = value * SCALE_DAMPENING;
//     self.update_combs();
//     self
// }

// pub fn freeze(&mut self, frozen: bool) -> &mut Self {
//     self.frozen = frozen;
//     self.update_combs();
//     self
// }

// pub fn wet(&mut self, value: f32) -> &mut Self {
//     self.wet = value * SCALE_WET;
//     self.update_wet_gains();
//     self
// }

// pub fn width(&mut self, value: f32) -> &mut Self {
//     self.width = value;
//     self.update_wet_gains();
//     self
// }

// fn update_wet_gains(&mut self) {
//     self.wet_gain = self.wet * (self.width / 2.0 + 0.5);
// }

// pub fn frozen(&mut self, frozen: bool) -> &mut Self {
//     self.frozen = frozen;
//     self.input_gain = if frozen { 0.0 } else { 1.0 };
//     self.update_combs();
//     self
// }

// pub fn room_size(&mut self, value: f32) -> &mut Self {
//     self.room_size = value * SCALE_ROOM + OFFSET_ROOM;
//     self.update_combs();
//     self
// }

// fn update_combs(&mut self) {
//     let (feedback, dampening) = if self.frozen {
//         (1.0, 0.0)
//     } else {
//         (self.room_size, self.dampening)
//     };

//     for o in self.rack.0.clone().iter_mut() {
//         if let Some(v) = o.as_any_mut().downcast_mut::<Comb>() {
//             v.feedback(feedback);
//             v.dampening(dampening);
//         }
//     }
// }

// pub fn dry(&mut self, value: f32) -> &mut Self {
//     self.dry = value;
//     self
// }
// }
