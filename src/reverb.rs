use super::{collections::*, dsp::*, filters::*};

// const FIXED_GAIN: f64 = 0.015;

const SCALE_WET: f64 = 3.0;
const SCALE_DAMPENING: f64 = 0.4;

const SCALE_ROOM: f64 = 0.28;
const OFFSET_ROOM: f64 = 0.7;

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

fn combs<W>(wave: ArcMutex<W>) -> ArcMutex<PolySynth<Comb<W>>>
where
    W: Signal + Send,
{
    let mut combs: Vec<ArcMutex<Comb<W>>> = Vec::new();
    let w2 = wave.clone();
    let w3 = wave.clone();
    let w4 = wave.clone();
    let w5 = wave.clone();
    let w6 = wave.clone();
    let w7 = wave.clone();
    let w8 = wave.clone();
    combs.push(Comb::<W>::wrapped(wave, COMB_TUNING_1));
    combs.push(Comb::<W>::wrapped(w2, COMB_TUNING_2));
    combs.push(Comb::<W>::wrapped(w3, COMB_TUNING_3));
    combs.push(Comb::<W>::wrapped(w4, COMB_TUNING_4));
    combs.push(Comb::<W>::wrapped(w5, COMB_TUNING_5));
    combs.push(Comb::<W>::wrapped(w6, COMB_TUNING_6));
    combs.push(Comb::<W>::wrapped(w7, COMB_TUNING_7));
    combs.push(Comb::<W>::wrapped(w8, COMB_TUNING_8));
    PolySynth::wrapped(combs, 1.0)
}

pub struct Freeverb<W>
where
    W: Signal + Send,
{
    allpasses: ArcMutex<AllPass<AllPass<AllPass<AllPass<PolySynth<Comb<W>>>>>>>,
    wet_gain: f64,
    wet: f64,
    width: f64,
    dry: f64,
    input_gain: f64,
    dampening: f64,
    room_size: f64,
    frozen: bool,
}

impl<W> Freeverb<W>
where
    W: Signal + Send,
{
    pub fn new(wave: ArcMutex<W>) -> Self {
        let combs = combs(wave);
        let allpasses = AllPass::wrapped(
            AllPass::wrapped(
                AllPass::wrapped(
                    AllPass::<PolySynth<Comb<W>>>::wrapped(combs, ALLPASS_TUNING_1),
                    ALLPASS_TUNING_2,
                ),
                ALLPASS_TUNING_3,
            ),
            ALLPASS_TUNING_4,
        );
        Freeverb {
            allpasses,
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

    pub fn wrapped(wave: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Freeverb::new(wave))
    }

    pub fn set_dampening(&mut self, value: f64) {
        self.dampening = value * SCALE_DAMPENING;
        self.update_combs();
    }

    pub fn set_freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.update_combs();
    }

    pub fn set_wet(&mut self, value: f64) {
        self.wet = value * SCALE_WET;
        self.update_wet_gains();
    }

    pub fn set_width(&mut self, value: f64) {
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

    pub fn set_room_size(&mut self, value: f64) {
        self.room_size = value * SCALE_ROOM + OFFSET_ROOM;
        self.update_combs();
    }

    fn update_combs(&mut self) {
        let (feedback, dampening) = if self.frozen {
            (1.0, 0.0)
        } else {
            (self.room_size, self.dampening)
        };

        for comb in self
            .allpasses
            .mtx()
            .wave
            .mtx()
            .wave
            .mtx()
            .wave
            .mtx()
            .wave
            .mtx()
            .waves
            .iter_mut()
        {
            comb.mtx().feedback = feedback;
            comb.mtx().dampening = dampening;
        }
    }

    pub fn set_dry(&mut self, value: f64) {
        self.dry = value;
    }
}

impl<W> Signal for Freeverb<W>
where
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let input = self.allpasses.signal(sample_rate);
        let out = self.allpasses.signal(sample_rate);
        (out as f64 * self.wet_gain + input as f64 * self.dry) as f32
    }
}

impl<W> HasHz for Freeverb<W>
where
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        self.allpasses.hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.allpasses.modify_hz(f);
    }
}
