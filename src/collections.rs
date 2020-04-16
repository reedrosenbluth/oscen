use super::dsp::*;
pub struct Wave2<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
}

impl<V, W> Wave2<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> Self {
        Self { wave1, wave2 }
    }

    pub fn boxed(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Wave2 { wave1, wave2 })
    }
}

impl<V, W> Wave for Wave2<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        wave1.sample() + wave2.sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
    }
}
pub struct Wave3<U, V, W>
where
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<V>,
    pub wave3: ArcMutex<W>,
}

impl<U, V, W> Wave3<U, V, W>
where
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
        }
    }
}

impl<U, V, W> Wave for Wave3<U, V, W>
where
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        let wave3 = self.wave3.lock().unwrap();
        wave1.sample() + wave2.sample() + wave3.sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave3.lock().unwrap().update_phase(0.0, sample_rate);
    }
}
pub struct Wave4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<T>,
    pub wave2: ArcMutex<U>,
    pub wave3: ArcMutex<V>,
    pub wave4: ArcMutex<W>,
}

impl<T, U, V, W> Wave4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
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

impl<T, U, V, W> Wave for Wave4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        let wave3 = self.wave3.lock().unwrap();
        let wave4 = self.wave4.lock().unwrap();
        wave1.sample() + wave2.sample() + wave3.sample() + wave4.sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave3.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave4.lock().unwrap().update_phase(0.0, sample_rate);
    }
}
pub struct LerpWave<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub alpha: f32,
}

impl<V, W> LerpWave<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn boxed(wave1: ArcMutex<V>, wave2: ArcMutex<W>, alpha: f32) -> ArcMutex<Self> {
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

impl<V, W> Wave for LerpWave<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        (1. - self.alpha) * wave1.sample() + self.alpha * wave2.sample()
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
    }
}
pub struct PolyWave<W>
where
    W: Wave + Send,
{
    pub waves: Vec<ArcMutex<W>>,
    pub volume: f32,
}

impl<W> PolyWave<W>
where
    W: Wave + Send,
{
    pub fn new(waves: Vec<ArcMutex<W>>, volume: f32) -> Self {
        Self { waves, volume }
    }

    pub fn boxed(waves: Vec<ArcMutex<W>>, volume: f32) -> ArcMutex<Self> {
        arc(Self::new(waves, volume))
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
}

impl<W> Wave for PolyWave<W>
where
    W: Wave + Send,
{
    fn sample(&self) -> f32 {
        self.volume
            * self
                .waves
                .iter()
                .fold(0.0, |acc, x| acc + x.lock().unwrap().sample())
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().update_phase(0.0, sample_rate);
        }
    }
}

pub struct OneOf2<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<V>,
    pub wave2: ArcMutex<W>,
    pub playing: usize,
}

impl<V, W> OneOf2<V, W>
where
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            playing: 0,
        }
    }

    pub fn boxed(wave1: ArcMutex<V>, wave2: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2))
    }
}

impl<V, W> Wave for OneOf2<V, W>
where
    W: Wave + Send,
    V: Wave + Send,
{
    fn sample(&self) -> f32 {
        match self.playing {
            0 => self.wave1.lock().unwrap().sample(),
            1 => self.wave2.lock().unwrap().sample(),
            _ => self.wave1.lock().unwrap().sample(),
        }
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
    }
}

pub struct OneOf3<U, V, W>
where
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<U>,
    pub wave2: ArcMutex<V>,
    pub wave3: ArcMutex<W>,
    pub playing: usize,
}

impl<U, V, W> OneOf3<U, V, W>
where
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub fn new(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> Self {
        Self {
            wave1,
            wave2,
            wave3,
            playing: 0,
        }
    }

    pub fn boxed(wave1: ArcMutex<U>, wave2: ArcMutex<V>, wave3: ArcMutex<W>) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2, wave3))
    }
}

impl<U, V, W> Wave for OneOf3<U, V, W>
where
    U: Wave + Send,
    W: Wave + Send,
    V: Wave + Send,
{
    fn sample(&self) -> f32 {
        match self.playing {
            0 => self.wave1.lock().unwrap().sample(),
            1 => self.wave2.lock().unwrap().sample(),
            2 => self.wave3.lock().unwrap().sample(),
            _ => self.wave1.lock().unwrap().sample(),
        }
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave3.lock().unwrap().update_phase(0.0, sample_rate);
    }
}

pub struct OneOf4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
{
    pub wave1: ArcMutex<T>,
    pub wave2: ArcMutex<U>,
    pub wave3: ArcMutex<V>,
    pub wave4: ArcMutex<W>,
    pub playing: usize,
}

impl<T, U, V, W> OneOf4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    V: Wave + Send,
    W: Wave + Send,
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

    pub fn boxed(
        wave1: ArcMutex<T>,
        wave2: ArcMutex<U>,
        wave3: ArcMutex<V>,
        wave4: ArcMutex<W>,
    ) -> ArcMutex<Self> {
        arc(Self::new(wave1, wave2, wave3, wave4))
    }
}

impl<T, U, V, W> Wave for OneOf4<T, U, V, W>
where
    T: Wave + Send,
    U: Wave + Send,
    W: Wave + Send,
    V: Wave + Send,
{
    fn sample(&self) -> f32 {
        match self.playing {
            0 => self.wave1.lock().unwrap().sample(),
            1 => self.wave2.lock().unwrap().sample(),
            2 => self.wave3.lock().unwrap().sample(),
            3 => self.wave4.lock().unwrap().sample(),
            _ => self.wave1.lock().unwrap().sample(),
        }
    }

    fn update_phase(&mut self, _add: Phase, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave2.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave3.lock().unwrap().update_phase(0.0, sample_rate);
        self.wave4.lock().unwrap().update_phase(0.0, sample_rate);
    }
}
