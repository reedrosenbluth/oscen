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
    wave: Tag,
}

impl Freeverb {
    pub fn new(tag: Tag, wave: Tag) -> Self {
        Self { tag, wave }
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

// impl Signal for Freeverb {
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
    wave: Tag,
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
    pub fn new(wave: Tag) -> Self {
        Self {
            wave,
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

    pub fn rack(&self, rack: &mut Rack, controls: &mut Controls, buffers: &mut Buffers,) -> Arc<Freeverb> {
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
        let comb1_l = CombBuilder::new(self.wave, buffer, COMB_TUNING_L1);
    }
}

impl Freeverb {
    // pub fn new(tag: Tag, wave: Tag) -> Self {
    //     let comb1 = Comb::new(&mut id, input.tag(), COMB_TUNING_1).wrap();
    //     let comb2 = Comb::new(&mut id, input.tag(), COMB_TUNING_2).wrap();
    //     let comb3 = Comb::new(&mut id, input.tag(), COMB_TUNING_3).wrap();
    //     let comb4 = Comb::new(&mut id, input.tag(), COMB_TUNING_4).wrap();
    //     let comb5 = Comb::new(&mut id, input.tag(), COMB_TUNING_5).wrap();
    //     let comb6 = Comb::new(&mut id, input.tag(), COMB_TUNING_6).wrap();
    //     let comb7 = Comb::new(&mut id, input.tag(), COMB_TUNING_7).wrap();
    //     let comb8 = Comb::new(&mut id, input.tag(), COMB_TUNING_8).wrap();

    //     let combs = Mixer::new(
    //         &mut id,
    //         vec![
    //             comb1.tag(),
    //             comb2.tag(),
    //             comb3.tag(),
    //             comb4.tag(),
    //             comb5.tag(),
    //             comb6.tag(),
    //             comb7.tag(),
    //             comb8.tag(),
    //         ],
    //     )
    //     .wrap();

    //     let all1 = AllPass::new(&mut id, combs.tag(), ALLPASS_TUNING_1).wrap();
    //     let all2 = AllPass::new(&mut id, all1.tag(), ALLPASS_TUNING_2).wrap();
    //     let all3 = AllPass::new(&mut id, all2.tag(), ALLPASS_TUNING_3).wrap();
    //     let all4 = AllPass::new(&mut id, all3.tag(), ALLPASS_TUNING_4).wrap();
    //     let rack = Rack::new()
    //         .modules(vec![
    //             input.clone(),
    //             comb1,
    //             comb2,
    //             comb3,
    //             comb4,
    //             comb5,
    //             comb6,
    //             comb7,
    //             comb8,
    //             combs,
    //             all1,
    //             all2,
    //             all3,
    //             all4,
    //         ])
    //         .build();
    //     Freeverb {
    //         tag: id_gen.id(),
    //         wave,
    //         input,
    //         rack,
    //         wet_gain: 0.25,
    //         wet: 1.0,
    //         dry: 0.0,
    //         input_gain: 0.5,
    //         width: 0.5,
    //         dampening: 0.5,
    //         room_size: 0.5,
    //         frozen: false,
    //         out: 0.0,
    //     }
    // }

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

    fn update_combs(&mut self) {
        let (feedback, dampening) = if self.frozen {
            (1.0, 0.0)
        } else {
            (self.room_size, self.dampening)
        };

        for o in self.rack.0.clone().iter_mut() {
            if let Some(v) = o.as_any_mut().downcast_mut::<Comb>() {
                v.feedback(feedback);
                v.dampening(dampening);
            }
        }
    }

    // pub fn dry(&mut self, value: f32) -> &mut Self {
    //     self.dry = value;
    //     self
    // }
}

