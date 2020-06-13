use super::{filters::*, operators::*, signal::*};
use crate::{as_any_mut, std_signal};
use std::any::Any;

// const FIXED_GAIN: Real = 0.015;

const SCALE_WET: Real = 3.0;
const SCALE_DAMPENING: Real = 0.4;
const SCALE_ROOM: Real = 0.28;
const OFFSET_ROOM: Real = 0.7;

const COMB_TUNING_1: usize = 1116;
const COMB_TUNING_2: usize = 1188;
const COMB_TUNING_3: usize = 1277;
const COMB_TUNING_4: usize = 1356;
const COMB_TUNING_5: usize = 1422;
const COMB_TUNING_6: usize = 1491;
const COMB_TUNING_7: usize = 1557;
const COMB_TUNING_8: usize = 1617;

const ALLPASS_TUNING_1: usize = 556;
const ALLPASS_TUNING_2: usize = 441;
const ALLPASS_TUNING_3: usize = 341;
const ALLPASS_TUNING_4: usize = 225;

pub struct Freeverb {
    pub tag: Tag,
    pub wave: Tag,
    input: ArcMutex<Link>,
    rack: Rack,
    wet_gain: Real,
    wet: Real,
    width: Real,
    dry: Real,
    input_gain: Real,
    dampening: Real,
    room_size: Real,
    frozen: bool,
}

impl Freeverb {
    pub fn new(wave: Tag) -> Self {
        let input = Link::new().wrap();
        let comb1 = Comb::new(input.tag(), COMB_TUNING_1).wrap();
        let comb2 = Comb::new(input.tag(), COMB_TUNING_2).wrap();
        let comb3 = Comb::new(input.tag(), COMB_TUNING_3).wrap();
        let comb4 = Comb::new(input.tag(), COMB_TUNING_4).wrap();
        let comb5 = Comb::new(input.tag(), COMB_TUNING_5).wrap();
        let comb6 = Comb::new(input.tag(), COMB_TUNING_6).wrap();
        let comb7 = Comb::new(input.tag(), COMB_TUNING_7).wrap();
        let comb8 = Comb::new(input.tag(), COMB_TUNING_8).wrap();

        let combs = Mixer::new(vec![
            comb1.tag(),
            comb2.tag(),
            comb3.tag(),
            comb4.tag(),
            comb5.tag(),
            comb6.tag(),
            comb7.tag(),
            comb8.tag(),
        ]).wrap();

        let all1 = AllPass::new(combs.tag(), ALLPASS_TUNING_1).wrap();
        let all2 = AllPass::new(all1.tag(), ALLPASS_TUNING_2).wrap();
        let all3 = AllPass::new(all2.tag(), ALLPASS_TUNING_3).wrap();
        let all4 = AllPass::new(all3.tag(), ALLPASS_TUNING_4).wrap();
        let rack = Rack::new(vec![
            input.clone(),
            comb1,
            comb2,
            comb3,
            comb4,
            comb5,
            comb6,
            comb7,
            comb8,
            combs,
            all1,
            all2,
            all3,
            all4,
        ]);
        Freeverb {
            tag: mk_tag(),
            wave,
            input,
            rack,
            wet_gain: 0.5,
            wet: 1.0,
            dry: 0.0,
            input_gain: 0.5,
            width: 0.5,
            dampening: 0.5,
            room_size: 0.5,
            frozen: false,
        }
    }

    pub fn set_dampening(&mut self, value: Real) {
        self.dampening = value * SCALE_DAMPENING;
        self.update_combs();
    }

    pub fn set_freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.update_combs();
    }

    pub fn set_wet(&mut self, value: Real) {
        self.wet = value * SCALE_WET;
        self.update_wet_gains();
    }

    pub fn set_width(&mut self, value: Real) {
        self.width = value;
        self.update_wet_gains();
    }

    fn update_wet_gains(&mut self) {
        self.wet_gain = self.wet * (self.width / 2.0 + 0.5);
    }

    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.input_gain = if frozen { 0.0 } else { 1.0 };
        self.update_combs();
    }

    pub fn set_room_size(&mut self, value: Real) {
        self.room_size = value * SCALE_ROOM + OFFSET_ROOM;
        self.update_combs();
    }

    fn update_combs(&mut self) {
        let (feedback, dampening) = if self.frozen {
            (1.0, 0.0)
        } else {
            (self.room_size, self.dampening)
        };

        for o in self.rack.order.clone().iter_mut() {
            if let Some(v) = self
                .rack
                .nodes
                .get_mut(o)
                .unwrap()
                .module
                .as_any_mut()
                .downcast_mut::<Comb>()
            {
                v.feedback = feedback.into();
                v.dampening = dampening.into();
            }
        }
    }

    pub fn set_dry(&mut self, value: Real) {
        self.dry = value;
    }
}

impl Signal for Freeverb {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let inp = rack.output(self.wave);
        self.input.lock().unwrap().value = inp.into();
        let out = self.rack.signal(sample_rate);
        out * self.wet_gain + inp * self.dry
    }
}
