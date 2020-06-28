use super::signal::*;
use crate::{as_any_mut, std_signal};
use pitch_calc::{hz_from_letter_octave, LetterOctave};
use std::any::Any;

fn tick(clock: Real, seq_len: usize, bps: Real, sample_rate: Real) -> Real {
    let n = seq_len as Real;
    (clock + 1.0) % (sample_rate / bps * n) 
}

fn idx(clock: Real, bps: Real, sample_rate: Real) -> usize {
    (clock / sample_rate * bps) as usize
}

#[derive(Clone)]
pub struct PitchSeq {
    tag: Tag,
    sequence: Vec<LetterOctave>,
    bpm: In, // beats prer minute
    clock: Real,
}

impl PitchSeq {
    pub fn new() -> Self {
        PitchSeq {
            tag: mk_tag(),
            sequence: vec![],
            bpm: 480.into(),
            clock: 0.0,
        }
    }

    pub fn sequence(&mut self, arg: Vec<LetterOctave>) -> &mut Self {
        self.sequence = arg;
        self
    }

    pub fn bpm<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.bpm = arg.into();
        self
    }
}

impl Builder for PitchSeq {}

impl Signal for PitchSeq {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let bps = In::val(&rack, self.bpm) / 60.0;
        let idx = idx(self.clock, bps, sample_rate);
        self.clock = tick(self.clock, self.sequence.len(), bps, sample_rate);
        let LetterOctave(letter, octave) = self.sequence[idx];
        hz_from_letter_octave(letter, octave) as Real
    }
}

#[derive(Clone)]
pub struct GateSeq {
    tag: Tag,
    sequence: Vec<bool>,
    bpm: In, // beats per minute
    clock: Real,
}

impl GateSeq {
    pub fn new() -> Self {
        GateSeq {
            tag: mk_tag(),
            sequence: vec![],
            bpm: 480.into(),
            clock: 0.0,
        }
    }

    pub fn sequence(&mut self, arg: Vec<bool>) -> &mut Self {
        self.sequence = arg;
        self
    }

    pub fn bpm<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.bpm = arg.into();
        self
    }
}

impl Builder for GateSeq {}

impl Signal for GateSeq {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let bps = In::val(&rack, self.bpm) / 60.0;
        let idx = idx(self.clock, bps, sample_rate);
        self.clock = tick(self.clock, self.sequence.len(), bps, sample_rate);
        self.sequence[idx] as usize as Real

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::signals;
    use pitch_calc::Letter;

    #[test]
    fn pitch_seq() {
        let seq = vec![
            LetterOctave(Letter::A, 2),
            LetterOctave(Letter::A, 3),
            LetterOctave(Letter::A, 4),
            LetterOctave(Letter::A, 5),
        ];
        let mut ps = PitchSeq::new().sequence(seq).build();
        let sigs = signals(&mut ps, 0, 16, 4.0);
        let s0 = sigs[0].1.round();
        let s1 = sigs[1].1.round();
        let s2 = sigs[2].1.round();
        let s14 = sigs[14].1.round();

        assert_eq!(s0, 110.0, "Expected 110 actual: {}", s0);
        assert_eq!(s1, 110.0, "Expected 110 actual: {}", s1);
        assert_eq!(s2, 220.0, "Expected 220 actual: {}", s2);
        assert_eq!(s14, 880.0, "Expected 880 actual: {}", s14);
    }

    #[test]
    fn gate_seq() {
        let seq = vec![true, false, true, false];
        let mut ps = GateSeq::new().sequence(seq).build();
        let sigs = signals(&mut ps, 0, 16, 4.0);
        let s0 = sigs[0].1;
        let s1 = sigs[1].1;
        let s2 = sigs[2].1;
        let s14 = sigs[14].1;

        assert_eq!(s0, 1.0, "Expected true actual: {}", s0);
        assert_eq!(s1, 1.0, "Expected true actual: {}", s1);
        assert_eq!(s2, 0.0, "Expected false actual: {}", s2);
        assert_eq!(s14, 0.0, "Expected true actual: {}", s14);
    }
}
