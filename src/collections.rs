use super::dsp::*;
use derive_more::Constructor;
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

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
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

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
        self.wave3.lock().unwrap().update_phase(sample_rate);
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
    pub fn new(wave1: ArcMutex<T>, wave2: ArcMutex<U>, wave3: ArcMutex<V>, wave4: ArcMutex<W>) -> Self {
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

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
        self.wave3.lock().unwrap().update_phase(sample_rate);
        self.wave4.lock().unwrap().update_phase(sample_rate);
    }
}
#[derive(Constructor)]
pub struct LerpWave {
    pub wave1: ArcWave,
    pub wave2: ArcWave,
    pub alpha: f32,
}

impl LerpWave {
    pub fn boxed(wave1: ArcWave, wave2: ArcWave, alpha: f32) -> ArcMutex<Self> {
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

impl Wave for LerpWave {
    fn sample(&self) -> f32 {
        let wave1 = self.wave1.lock().unwrap();
        let wave2 = self.wave2.lock().unwrap();
        (1. - self.alpha) * wave1.sample() + self.alpha * wave2.sample()
    }

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
    }
}
pub struct PolyWave {
    pub waves: Vec<ArcWave>,
    pub volume: f32,
}

impl PolyWave {
    pub fn new(waves: Vec<ArcWave>, volume: f32) -> Self {
        Self { waves, volume }
    }

    pub fn boxed(waves: Vec<ArcWave>, volume: f32) -> ArcMutex<Self> {
        arc(Self::new(waves, volume))
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
}

impl Wave for PolyWave {
    fn sample(&self) -> f32 {
        self.volume
            * self
                .waves
                .iter()
                .fold(0.0, |acc, x| acc + x.lock().unwrap().sample())
    }

    fn update_phase(&mut self, sample_rate: f64) {
        for wave in self.waves.iter_mut() {
            wave.lock().unwrap().update_phase(sample_rate);
        }
    }
}

pub struct OneOf3Wave<U, V, W>
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

impl<U, V, W> OneOf3Wave<U, V, W>
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

impl<U, V, W> Wave for OneOf3Wave<U, V, W>
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

    fn update_phase(&mut self, sample_rate: f64) {
        self.wave1.lock().unwrap().update_phase(sample_rate);
        self.wave2.lock().unwrap().update_phase(sample_rate);
        self.wave3.lock().unwrap().update_phase(sample_rate);
    }
}
