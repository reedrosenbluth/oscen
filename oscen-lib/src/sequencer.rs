use super::signal::*;
use crate::{as_any_mut, std_signal};
use pitch_calc::{hz_from_letter_octave, Letter, LetterOctave, Octave};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::any::Any;

fn tick(clock: Real, seq_len: usize, bps: Real, sample_rate: Real) -> Real {
    let n = seq_len as Real;
    (clock + 1.0) % (sample_rate / bps * n)
}

fn idx(clock: Real, bps: Real, sample_rate: Real) -> usize {
    (clock / sample_rate * bps) as usize
}

#[derive(Clone)]
pub struct Note {
    pitch: LetterOctave,
    gate: bool,
}

impl Note {
    pub fn new(letter: Letter, octave: Octave, gate: bool) -> Self {
        Self {
            pitch: LetterOctave(letter, octave),
            gate,
        }
    }
}

#[derive(Clone)]
pub struct Sequencer {
    sequence: Vec<Note>,
    bpm: In, // beats prer minute
    clock: Real,
}

impl Sequencer {
    pub fn new() -> Self {
        Sequencer {
            sequence: vec![],
            bpm: 120.into(),
            clock: 0.0,
        }
    }

    pub fn sequence(&mut self, arg: Vec<Note>) -> &mut Self {
        self.sequence = arg;
        self
    }

    pub fn bpm<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.bpm = arg.into();
        self
    }
}

impl Builder for Sequencer {}

#[derive(Clone)]
pub struct PitchSeq {
    tag: Tag,
    seq: Sequencer,
    out: Real,
}

impl PitchSeq {
    pub fn new(id_gen: &mut IdGen, seq: Sequencer) -> Self {
        Self {
            tag: id_gen.id(),
            seq,
            out: 0.0,
        }
    }
}

impl Builder for PitchSeq {}

impl Signal for PitchSeq {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let bps = In::val(&rack, self.seq.bpm) / 60.0;
        let idx = idx(self.seq.clock, bps, sample_rate);
        if idx == 0 {
            let mut rng = thread_rng();
            self.seq.sequence.shuffle(&mut rng);
        }
        self.seq.clock = tick(self.seq.clock, self.seq.sequence.len(), bps, sample_rate);
        let LetterOctave(letter, octave) = self.seq.sequence[idx].pitch;
        self.out = hz_from_letter_octave(letter, octave) as Real;
        self.out
    }
}

#[derive(Clone)]
pub struct GateSeq {
    tag: Tag,
    seq: Sequencer,
    out: Real,
}

impl GateSeq {
    pub fn new(id_gen: &mut IdGen, seq: Sequencer) -> Self {
        Self {
            tag: id_gen.id(),
            seq,
            out: 0.0,
        }
    }
}

impl Builder for GateSeq {}

impl Signal for GateSeq {
    std_signal!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        let bps = In::val(&rack, self.seq.bpm) / 60.0;
        let idx = idx(self.seq.clock, bps, sample_rate);
        self.seq.clock = tick(self.seq.clock, self.seq.sequence.len(), bps, sample_rate);
        self.out = self.seq.sequence[idx].gate as usize as Real;
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::signals;
    use pitch_calc::Letter;

    #[test]
    fn pitch_seq() {
        let notes: Vec<Note> = vec![
            Note::new(Letter::A, 2, true),
            Note::new(Letter::A, 3, false),
            Note::new(Letter::A, 4, true),
            Note::new(Letter::A, 5, false),
        ];
        let seq: Sequencer = Sequencer::new().sequence(notes).build();
        let mut id_gen = IdGen::new();
        let mut ps = PitchSeq::new(&mut id_gen, seq);
        let sigs = signals(&mut ps, 0, 16, 4.0);
        let s0 = sigs[0].1.round();
        let s1 = sigs[1].1.round();
        let s2 = sigs[2].1.round();
        let s14 = sigs[14].1.round();

        assert_eq!(s0, 110.0, "0 - Expected 110 actual: {}", s0);
        assert_eq!(s1, 110.0, "1 - Expected 110 actual: {}", s1);
        assert_eq!(s2, 220.0, "2 - Expected 220 actual: {}", s2);
        assert_eq!(s14, 880.0, "14 - Expected 880 actual: {}", s14);
    }

    #[test]
    fn gate_seq() {
        let notes: Vec<Note> = vec![
            Note::new(Letter::A, 2, true),
            Note::new(Letter::A, 3, false),
            Note::new(Letter::A, 4, true),
            Note::new(Letter::A, 5, false),
        ];
        let seq: Sequencer = Sequencer::new().sequence(notes).build();
        let mut id_gen = IdGen::new();
        let mut ps = GateSeq::new(&mut id_gen, seq);
        let sigs = signals(&mut ps, 0, 16, 4.0);
        let s0 = sigs[0].1;
        let s1 = sigs[1].1;
        let s2 = sigs[2].1;
        let s14 = sigs[14].1;

        assert_eq!(s0, 1.0, "0 - Expected true actual: {}", s0);
        assert_eq!(s1, 1.0, "1 - Expected true actual: {}", s1);
        assert_eq!(s2, 0.0, "2 - Expected false actual: {}", s2);
        assert_eq!(s14, 0.0, "14 - Expected true actual: {}", s14);
    }
}
