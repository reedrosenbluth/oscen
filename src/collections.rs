use super::dsp::*;
pub struct Wave2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
}

impl<V, W> Wave2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> Self {
        Self { wave1, wave2 }
    }

    pub fn wrapped(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Wave2 { wave1, wave2 })
    }
}

impl<V, W> Signal for Wave2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let mut wave1 = self.wave1.lock().unwrap();
        let mut wave2 = self.wave2.lock().unwrap();
        wave1.signal_add(sample_rate, add) + wave2.signal_add(sample_rate, add)
    }
}

pub struct Wave3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<V>,
    pub wave3: ArcMutex<W>,
}

impl<U, V, W> Wave3<U, V, W>
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

impl<U, V, W> Signal for Wave3<U, V, W>
where
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let mut wave1 = self.wave1.lock().unwrap();
        let mut wave2 = self.wave2.lock().unwrap();
        let mut wave3 = self.wave3.lock().unwrap();
        wave1.signal_add(sample_rate, add)
            + wave2.signal_add(sample_rate, add)
            + wave3.signal_add(sample_rate, add)
    }
}
pub struct Wave4<T, U, V, W>
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

impl<T, U, V, W> Wave4<T, U, V, W>
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

impl<T, U, V, W> Signal for Wave4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let mut wave1 = self.wave1.lock().unwrap();
        let mut wave2 = self.wave2.lock().unwrap();
        let mut wave3 = self.wave3.lock().unwrap();
        let mut wave4 = self.wave4.lock().unwrap();
        wave1.signal_add(sample_rate, add)
            + wave2.signal_add(sample_rate, add)
            + wave3.signal_add(sample_rate, add)
            + wave4.signal_add(sample_rate, add)
    }
}
pub struct LerpWave<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub alpha: f32,
}

impl<V, W> LerpWave<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub fn wrapped(wave1: ArcMutex<V>, wave2: ArcMutex<W>, alpha: f32) -> ArcMutex<Self> {
        arc(LerpWave {
            wave1,
            wave2,
            alpha,
        })
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }
}

impl<V, W> Signal for LerpWave<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        let mut wave1 = self.wave1.lock().unwrap();
        let mut wave2 = self.wave2.lock().unwrap();
        (1. - self.alpha) * wave1.signal_add(sample_rate, add)
            + self.alpha * wave2.signal_add(sample_rate, add)
    }
}
pub struct PolyWave<W>
where
    W: Signal + Send,
{
    pub waves: Vec<ArcMutex<W>>,
    pub volume: f32,
}

impl<W> PolyWave<W>
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

impl<W> Signal for PolyWave<W>
where
    W: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        self.volume
            * self.waves.iter().fold(0.0, |acc, x| {
                acc + x.lock().unwrap().signal_add(sample_rate, add)
            })
    }
}

pub struct OneOf2<V, W>
where
    V: Signal + Send,
    W: Signal + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub playing: usize,
}

impl<V, W> OneOf2<V, W>
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

impl<V, W> Signal for OneOf2<V, W>
where
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        match self.playing {
            0 => self.wave1.lock().unwrap().signal_add(sample_rate, add),
            1 => self.wave2.lock().unwrap().signal_add(sample_rate, add),
            _ => self.wave1.lock().unwrap().signal_add(sample_rate, add),
        }
    }
}

pub struct OneOf3<U, V, W>
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

impl<U, V, W> OneOf3<U, V, W>
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

impl<U, V, W> Signal for OneOf3<U, V, W>
where
    U: Signal + Send,
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        match self.playing {
            0 => self.wave1.lock().unwrap().signal_add(sample_rate, add),
            1 => self.wave2.lock().unwrap().signal_add(sample_rate, add),
            2 => self.wave3.lock().unwrap().signal_add(sample_rate, add),
            _ => self.wave1.lock().unwrap().signal_add(sample_rate, add),
        }
    }
}

pub struct OneOf4<T, U, V, W>
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

impl<T, U, V, W> OneOf4<T, U, V, W>
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

impl<T, U, V, W> Signal for OneOf4<T, U, V, W>
where
    T: Signal + Send,
    U: Signal + Send,
    W: Signal + Send,
    V: Signal + Send,
{
    fn signal_add(&mut self, sample_rate: f64, add: Phase) -> Amp {
        match self.playing {
            0 => self.wave1.lock().unwrap().signal_add(sample_rate, add),
            1 => self.wave2.lock().unwrap().signal_add(sample_rate, add),
            2 => self.wave3.lock().unwrap().signal_add(sample_rate, add),
            3 => self.wave4.lock().unwrap().signal_add(sample_rate, add),
            _ => self.wave1.lock().unwrap().signal_add(sample_rate, add),
        }
    }
}
