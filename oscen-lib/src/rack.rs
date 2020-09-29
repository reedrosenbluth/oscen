use arr_macro::arr;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

pub type SignalFn = fn(f32, f32) -> f32;

pub const MAX_CONTROLS: usize = 32;
pub const MAX_OUTPUTS: usize = 32;
pub const MAX_STATE: usize = 64;
// Must be changed by hand in Buffers due to limitaion of arr! marcro
pub const MAX_MODULES: usize = 1024;

/// Unique identifier for each Synth Module.
#[derive(Copy, Clone, Debug)]
pub struct Tag(pub usize);

impl Tag {
    pub fn new(t: usize) -> Self {
        Self(t)
    }
    fn get(&self) -> usize {
        self.0
    }
}

impl From<Tag> for usize {
    fn from(t: Tag) -> Self {
        t.0
    }
}

impl From<usize> for Tag {
    fn from(u: usize) -> Self {
        Self(u)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Control {
    V(Tag, usize),
    F(f32),
    B(bool),
    I(usize),
}

impl Control {
    pub fn idx(&self) -> usize {
        match self {
            Control::I(u) => *u,
            c => panic!("Expecting I variant, not {:?}", c),
        }
    }
}

impl From<f32> for Control {
    fn from(x: f32) -> Self {
        Control::F(x)
    }
}

impl From<usize> for Control {
    fn from(u: usize) -> Self {
        Control::I(u)
    }
}

impl From<bool> for Control {
    fn from(b: bool) -> Self {
        Control::B(b)
    }
}

impl From<Tag> for Control {
    fn from(t: Tag) -> Self {
        Control::V(t, 0)
    }
}

#[derive(Copy, Clone)]
pub struct Controls([[Control; MAX_CONTROLS]; MAX_MODULES]);

impl Controls {
    pub fn new() -> Self {
        Controls([[0.0.into(); MAX_CONTROLS]; MAX_MODULES])
    }
    pub fn controls<T: Into<usize>>(&self, tag: T) -> &[Control] {
        self.0[tag.into()].as_ref()
    }
    pub fn controls_mut<T: Into<usize>>(&mut self, tag: T) -> &mut [Control] {
        self.0[tag.into()].as_mut()
    }
}

impl<T> Index<(T, usize)> for Controls
where
    T: Into<Tag>,
{
    type Output = Control;
    fn index(&self, index: (T, usize)) -> &Self::Output {
        &self.controls(index.0.into())[index.1]
    }
}

impl<T> IndexMut<(T, usize)> for Controls
where
    T: Into<Tag>,
{
    fn index_mut(&mut self, index: (T, usize)) -> &mut Self::Output {
        &mut self.controls_mut(index.0.into())[index.1]
    }
}

#[derive(Copy, Clone)]
pub struct Outputs([[f32; MAX_OUTPUTS]; MAX_MODULES]);

impl Outputs {
    pub fn new() -> Self {
        Outputs([[0.0; MAX_OUTPUTS]; MAX_MODULES])
    }

    pub fn outputs<T: Into<usize>>(&self, tag: T) -> &[f32] {
        self.0[tag.into()].as_ref()
    }
    
    pub fn outputs_mut<T: Into<usize>>(&mut self, tag: T) -> &mut [f32] {
        self.0[tag.into()].as_mut()
    }

    pub fn value(&self, ctrl: Control) -> Option<f32> {
        match ctrl {
            Control::F(p) => Some(p),
            Control::V(n, i) => Some(self.0[n.get()][i]),
            _ => None,
        }
    }

    pub fn integer(&self, ctrl: Control) -> Option<usize> {
        match ctrl {
            Control::I(n) => Some(n),
            _ => None,
        }
    }

    pub fn boolean(&self, ctrl: Control) -> Option<bool> {
        match  ctrl {
            Control::B(b) => Some(b),
            _ => None,
        }
    }
}

impl<T> Index<(T, usize)> for Outputs
where
    T: Into<Tag>,
{
    type Output = f32;
    fn index(&self, index: (T, usize)) -> &Self::Output {
        &self.outputs(index.0.into())[index.1]
    }
}

impl<T> IndexMut<(T, usize)> for Outputs
where
    T: Into<Tag>,
{
    fn index_mut(&mut self, index: (T, usize)) -> &mut Self::Output {
        &mut self.outputs_mut(index.0.into())[index.1]
    }
}

#[derive(Copy, Clone)]
pub struct State([[f32; MAX_STATE]; MAX_MODULES]);

impl State {
    pub fn new() -> Self {
        State([[0.0; MAX_STATE]; MAX_MODULES])
    }
    pub fn state<T: Into<usize>>(&self, tag: T) -> &[f32] {
        self.0[tag.into()].as_ref()
    }
    pub fn state_mut<T: Into<usize>>(&mut self, tag: T) -> &mut [f32] {
        self.0[tag.into()].as_mut()
    }
}

impl<T> Index<(T, usize)> for State
where
    T: Into<Tag>,
{
    type Output = f32;
    fn index(&self, index: (T, usize)) -> &Self::Output {
        &self.state(index.0.into())[index.1]
    }
}

impl<T> IndexMut<(T, usize)> for State
where
    T: Into<Tag>,
{
    fn index_mut(&mut self, index: (T, usize)) -> &mut Self::Output {
        &mut self.state_mut(index.0.into())[index.1]
    }
}
/// Circular buffer
#[derive(Clone)]
pub struct RingBuffer<T = f32> {
    buffer: Vec<T>,
    write_pos: usize,
}

impl<T> RingBuffer<T>
where
    T: Clone + Default,
{
    pub fn new(write_pos: usize, buffer: Vec<T>) -> Self {
        Self { buffer, write_pos }
    }

    pub fn push(&mut self, v: T) {
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
        self.buffer[self.write_pos] = v;
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn set_write_pos(&mut self, wp: usize) {
        self.write_pos = wp % self.buffer.len();
    }

    pub fn read_pos(&self, delay: f32) -> f32 {
        let n = self.buffer.len() as f32;
        let mut rp = self.write_pos as f32 - delay;
        while rp >= n {
            rp -= n;
        }
        while rp < 0.0 {
            rp += n;
        }
        rp
    }
}

impl<T> RingBuffer<T>
where
    T: Copy + Default,
{
    pub fn get(&self, delay: f32) -> T {
        let rp = self.read_pos(delay).trunc() as usize;
        self.buffer[rp]
    }

    pub fn get_offset(&self, delay: f32, offset: i32) -> T {
        let n = self.buffer.len() as i32;
        let rp = self.read_pos(delay).trunc() as usize;
        let mut offset = offset;
        while offset < 0 {
            offset += n;
        }
        let i = (rp + offset as usize) % n as usize;
        self.buffer[i]
    }

    pub fn get_max_delay(&self) -> T {
        self.get(self.buffer.len() as f32 - 1.0)
    }

    pub fn resize(&mut self, n: usize) {
        self.buffer.resize_with(n, Default::default);
    }
}

impl RingBuffer {
    pub fn new32(sample_rate: f32) -> Self {
        let buffer = vec![0.0; sample_rate as usize];
        Self::new(0, buffer)
    }

    pub fn get_linear(&self, delay: f32) -> f32 {
        let rp = self.read_pos(delay);
        let f = rp - rp.trunc();
        (1.0 - f) * self.get(delay) + f * self.get_offset(delay, 1)
    }

    /// Hermite cubic polynomial interpolation.
    pub fn get_cubic(&self, delay: f32) -> f32 {
        let v0 = self.get_offset(delay, -1);
        let v1 = self.get(delay);
        let v2 = self.get_offset(delay, 1);
        let v3 = self.get_offset(delay, 2);
        let f = self.read_pos(delay) - self.read_pos(delay).trunc();
        let a1 = 0.5 * (v2 - v0);
        let a2 = v0 - 2.5 * v1 + 2.0 * v2 - 0.5 * v3;
        let a3 = 0.5 * (v3 - v0) + 1.5 * (v1 - v2);
        a3 * f * f * f + a2 * f * f + a1 * f + v1
    }
}

impl<T> Default for RingBuffer<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            write_pos: 0,
        }
    }
}
#[derive(Clone)]
pub struct Buffers([RingBuffer; MAX_MODULES]);

impl Buffers {
    pub fn new() -> Self {
        Buffers(arr![Default::default(); 1024])
    }
    pub fn buffers<T: Into<usize>>(&self, tag: T) -> &RingBuffer {
        &self.0[tag.into()]
    }
    pub fn set_buffer(&mut self, tag: Tag, buffer: RingBuffer) {
        self.0[tag.get()] = buffer;
    }
    pub fn buffers_mut<T: Into<usize>>(&mut self, tag: T) -> &mut RingBuffer {
        &mut self.0[tag.into()]
    }
}

/// Synth modules must implement the Signal trait. In fact one could define a
/// synth module as a struct that implements `Signal`.
pub trait Signal {
    /// Synth Modules are required to have a tag to be used as inputs to other
    /// modules.
    fn tag(&self) -> Tag;
    fn modify_tag(&mut self, f: fn(Tag) -> Tag);
    /// Responsible for updating the any inputs including `phase` and returning the next signal
    /// output.
    fn signal(
        &self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    );
}

/// A macro to reduce the boiler plate of creating a Synth Module by implementing
/// `tag` and `modify_tag`.
#[macro_export]
macro_rules! tag {
    () => {
        fn tag(&self) -> Tag {
            self.tag
        }
        fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
            self.tag = f(self.tag);
        }
    };
}

/// A Rack is a topologically sorted `Array` of Synth Modules. A synth is one or
/// more racks.
pub struct Rack(Vec<Arc<dyn Signal + Send + Sync>>);

impl Rack {
    pub fn new() -> Self {
        Rack(vec![])
    }
    pub fn num_modules(&self) -> usize {
        self.0.len()
    }
    pub fn push(&mut self, module: Arc<dyn Signal + Send + Sync>) {
        self.0.push(module);
    }
    /// Call the `signal` function for each module in turn returning the vector
    /// of outpts in the last module.
    pub fn play(
        &mut self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    ) -> [f32; MAX_OUTPUTS] {
        let n = self.0.len() - 1;
        for module in self.0.iter() {
            module.signal(controls, state, outputs, buffers, sample_rate);
        }
        outputs.0[n]
    }
    /// Like play but only returns the sample in `outputs[0].
    pub fn mono(
        &mut self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        buffers: &mut Buffers,
        sample_rate: f32,
    ) -> f32 {
        self.play(controls, state, outputs, buffers, sample_rate)[0]
    }
}

/// Generate the Environment variables needed for the synth.
pub fn tables() -> (Rack, Box<Controls>, Box<State>, Box<Outputs>, Box<Buffers>) {
    (
        Rack::new(),
        Box::new(Controls::new()),
        Box::new(State::new()),
        Box::new(Outputs::new()),
        Box::new(Buffers::new()),
    )
}

#[macro_export]
macro_rules! build {
    ($field:ident) => {
        pub fn $field<T: Into<Control>>(&mut self, value: T) -> &mut Self {
            self.$field = value.into();
            self
        }
    }
}

#[macro_export]
macro_rules! props {
    ($field:ident, $set:ident, $n:expr) => {
        pub fn $field(&self, controls: &Controls, outputs: &Outputs) -> f32 {
            let inp = controls[(self.tag, $n)];
            outputs.value(inp).unwrap()
        }
        pub fn $set(&self, controls: &mut Controls, value: Control) {
            controls[(self.tag, $n)] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_buffer() {
        let mut rb = RingBuffer::new32(10.0);
        let delay = 2.25;
        let result = rb.get(delay);
        assert_eq!(result, 0.0, "get returned {}, expected 0.0", result);
        for i in 0..=6 {
            rb.push(i as f32);
        }
        let result = rb.get(delay);
        assert_eq!(result, 3.0, "get returned {}, expected 3.0", result);
        let result = rb.get_linear(delay);
        assert_eq!(result, 3.75, "get_linear returned {}, expected 3.5", result);
        let result = rb.get_cubic(delay);
        assert_eq!(result, 3.75, "get_cubic returned {}, expected 3.75", result);
    }
}
