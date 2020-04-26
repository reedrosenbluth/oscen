use super::dsp::*;

pub struct Synth2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
}

impl<V, W> Synth2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> Self {
        Self { wave1, wave2 }
    }

    pub fn wrapped(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Synth2 { wave1, wave2 })
    }
}

impl<V, W> Signal for Synth2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let mut wave1 = self.wave1.mtx();
        let mut wave2 = self.wave2.mtx();
        wave1.signal(sample_rate) + wave2.signal(sample_rate)
    }
}

impl<V, W> HasHz for Synth2<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        0.0
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave1.mtx().modify_hz(f);
        self.wave2.mtx().modify_hz(f);
    }
}

pub struct Synth3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<V>,
    pub wave3: ArcMutex<W>,
}

impl<U, V, W> Synth3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
        }
    }
}

impl<U, V, W> Signal for Synth3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let mut wave1 = self.wave1.mtx();
        let mut wave2 = self.wave2.mtx();
        let mut wave3 = self.wave3.mtx();
        wave1.signal(sample_rate) + wave2.signal(sample_rate) + wave3.signal(sample_rate)
    }
}

impl<U, V, W> HasHz for Synth3<U, V, W>
where
    U: Signal + HasHz + Send,
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        0.0
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave1.mtx().modify_hz(f);
        self.wave2.mtx().modify_hz(f);
        self.wave3.mtx().modify_hz(f);
    }
}

pub struct Synth4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<T>,
    pub wave2: ArcMutex<U>,
    pub wave3: ArcMutex<V>,
    pub wave4: ArcMutex<W>,
}

impl<T, U, V, W> Synth4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(
        wave1: ArcMutex<T>,
        wave2: ArcMutex<U>,
        wave3: ArcMutex<V>,
        wave4: ArcMutex<W>,
    ) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
            wave4,
        }
    }
}

impl<T, U, V, W> Signal for Synth4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let mut wave1 = self.wave1.mtx();
        let mut wave2 = self.wave2.mtx();
        let mut wave3 = self.wave3.mtx();
        let mut wave4 = self.wave4.mtx();
        wave1.signal(sample_rate)
            + wave2.signal(sample_rate)
            + wave3.signal(sample_rate)
            + wave4.signal(sample_rate)
    }
}

impl<T, U, V, W> HasHz for Synth4<T, U, V, W>
where
    T: Signal + HasHz + Send,
    U: Signal + HasHz + Send,
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        0.0
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave1.mtx().modify_hz(f);
        self.wave2.mtx().modify_hz(f);
        self.wave3.mtx().modify_hz(f);
        self.wave3.mtx().modify_hz(f);
    }
}
pub struct LerpSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub alpha: f32,
}

impl<V, W> LerpSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>, alpha: f32) -> Self {
        Self {
            wave1,
            wave2,
            alpha,
        }
    }

    pub fn wrapped(wave1: ArcMutex<V>, wave2: ArcMutex<W>, alpha: f32) -> ArcMutex<Self> {
        arc(LerpSynth {
            wave1,
            wave2,
            alpha,
        })
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }
}

impl<V, W> Signal for LerpSynth<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        let mut wave1 = self.wave1.mtx();
        let mut wave2 = self.wave2.mtx();
        (1. - self.alpha) * wave1.signal(sample_rate) + self.alpha * wave2.signal(sample_rate)
    }
}

impl<V, W> HasHz for LerpSynth<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        (1.0 - self.alpha as f64) * self.wave1.mtx().hz() + self.alpha as f64 * self.wave2.mtx().hz()
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        self.wave1.mtx().modify_hz(f);
        self.wave2.mtx().modify_hz(f);
    }
}
pub struct PolySynth<W>
where
    W: Signal + Send,
{
    pub waves: Vec<ArcMutex<W>>,
    pub volume: f32,
}

impl<W> PolySynth<W>
where
    W: Signal + Send,
{
    pub fn new(waves: Vec<ArcMutex<W>>, volume: f32) -> Self {
        Self { waves, volume }
    }

    pub fn wrapped(waves: Vec<ArcMutex<W>>, volume: f32) -> ArcMutex<Self> {
        arc(Self::new(waves, volume))
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
}

impl<W> Signal for PolySynth<W>
where
    W: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        self.volume
            * self
                .waves
                .iter()
                .fold(0.0, |acc, x| acc + x.mtx().signal(sample_rate))
    }
}

impl<W> HasHz for PolySynth<W>
where
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        0.0
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        for w in self.waves.iter_mut() {
            w.mtx().modify_hz(f);
        }
    }
}

pub struct Variant2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub playing: usize,
}

impl<V, W> Variant2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            playing: 0,
        }
    }

    pub fn wrapped(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2))
    }
}

impl<V, W> Signal for Variant2<V, W>
where
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        match self.playing {
            0 => self.wave1.mtx().signal(sample_rate),
            1 => self.wave2.mtx().signal(sample_rate),
            _ => self.wave1.mtx().signal(sample_rate),
        }
    }
}

impl<V, W> HasHz for Variant2<V, W>
where
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        match self.playing {
            0 => self.wave1.mtx().hz(),
            1 => self.wave2.mtx().hz(),
            _ => self.wave1.mtx().hz(),
        }
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        match self.playing {
            0 => self.wave1.mtx().modify_hz(f),
            1 => self.wave2.mtx().modify_hz(f),
            _ => self.wave1.mtx().modify_hz(f),
        }
    }
}

pub struct Variant3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<V>,
    pub wave3: ArcMutex<W>,
    pub playing: usize,
}

impl<U, V, W> Variant3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
            playing: 0,
        }
    }

    pub fn wrapped(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2, wave3))
    }
}

impl<U, V, W> Signal for Variant3<U, V, W>
where
    U: Signal + Send,
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        match self.playing {
            0 => self.wave1.mtx().signal(sample_rate),
            1 => self.wave2.mtx().signal(sample_rate),
            2 => self.wave3.mtx().signal(sample_rate),
            _ => self.wave1.mtx().signal(sample_rate),
        }
    }
}
impl<U, V, W> HasHz for Variant3<U, V, W>
where
    U: Signal + HasHz + Send,
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        match self.playing {
            0 => self.wave1.mtx().hz(),
            1 => self.wave2.mtx().hz(),
            2 => self.wave3.mtx().hz(),
            _ => self.wave1.mtx().hz(),
        }
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        match self.playing {
            0 => self.wave1.mtx().modify_hz(f),
            1 => self.wave2.mtx().modify_hz(f),
            2 => self.wave3.mtx().modify_hz(f),
            _ => self.wave1.mtx().modify_hz(f),
        }
    }
}

pub struct Variant4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<T>,
    pub wave2: ArcMutex<U>,
    pub wave3: ArcMutex<V>,
    pub wave4: ArcMutex<W>,
    pub playing: usize,
}

impl<T, U, V, W> Variant4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(
        wave1: ArcMutex<T>,
        wave2: ArcMutex<U>,
        wave3: ArcMutex<V>,
        wave4: ArcMutex<W>,
    ) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
            wave4,
            playing: 0,
        }
    }

    pub fn wrapped(
        wave1: ArcMutex<T>,
        wave2: ArcMutex<U>,
        wave3: ArcMutex<V>,
        wave4: ArcMutex<W>,
    ) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2, wave3, wave4))
    }
}

impl<T, U, V, W> Signal for Variant4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal(&mut self, sample_rate: f64) -> Amp {
        match self.playing {
            0 => self.wave1.mtx().signal(sample_rate),
            1 => self.wave2.mtx().signal(sample_rate),
            2 => self.wave3.mtx().signal(sample_rate),
            3 => self.wave4.mtx().signal(sample_rate),
            _ => self.wave1.mtx().signal(sample_rate),
        }
    }
}

impl<T, U, V, W> HasHz for Variant4<T, U, V, W>
where
    T: Signal + HasHz + Send,
    U: Signal + HasHz + Send,
    V: Signal + HasHz + Send,
    W: Signal + HasHz + Send,
{
    fn hz(&self) -> Hz {
        match self.playing {
            0 => self.wave1.mtx().hz(),
            1 => self.wave2.mtx().hz(),
            2 => self.wave3.mtx().hz(),
            3 => self.wave4.mtx().hz(),
            _ => self.wave1.mtx().hz(),
        }
    }

    fn modify_hz(&mut self, f: &dyn Fn(Hz) -> Hz) {
        match self.playing {
            0 => self.wave1.mtx().modify_hz(f),
            1 => self.wave2.mtx().modify_hz(f),
            2 => self.wave3.mtx().modify_hz(f),
            3 => self.wave4.mtx().modify_hz(f),
            _ => self.wave1.mtx().modify_hz(f),
        }
    }
}