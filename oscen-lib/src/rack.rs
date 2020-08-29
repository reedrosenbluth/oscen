use std::ops::{Index, IndexMut};
use std::sync::Arc;

pub type SignalFn = fn(f32, f32) -> f32;

pub const MAX_CONTROLS: usize = 32;
pub const MAX_OUTPUTS: usize = 32;
pub const MAX_STATE: usize = 64;
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

#[derive(Copy, Clone, Debug)]
pub enum In {
    Cv(Tag, usize),
    Fix(f32),
}

impl Default for In {
    fn default() -> Self {
        Self::Fix(0.0)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Control {
    V(In),
    B(bool),
    I(usize),
}

impl From<f32> for Control {
    fn from(x: f32) -> Self {
        Control::V(In::Fix(x))
    }
}

impl From<usize> for Control {
    fn from(u: usize) -> Self {
        Control::V(In::Fix(u as f32))
    }
}

impl From<bool> for Control {
    fn from(b: bool) -> Self {
        Control::B(b)
    }
}

impl From<Tag> for Control {
    fn from(t: Tag) -> Self {
        Control::V(In::Cv(t, 0))
    }
}

#[derive(Copy, Clone)]
pub struct Controls([[Control; MAX_CONTROLS]; MAX_MODULES]);

impl Controls {
    pub fn new() -> Self {
        Controls([[0.into(); MAX_CONTROLS]; MAX_MODULES])
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
            Control::V(In::Fix(p)) => Some(p),
            Control::V(In::Cv(n, i)) => Some(self.0[n.get()][i]),
            _ => None,
        }
    }
    pub fn integer(&self, ctrl: Control) -> Option<usize> {
        match ctrl {
            Control::I(n) => Some(n),
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
        sample_rate: f32,
    ) -> [f32; MAX_OUTPUTS] {
        let n = self.0.len() - 1;
        for module in self.0.iter() {
            module.signal(controls, state, outputs, sample_rate);
        }
        outputs.0[n]
    }
    /// Like play but only returns the sample in `outputs[0].
    pub fn mono(
        &mut self,
        controls: &Controls,
        state: &mut State,
        outputs: &mut Outputs,
        sample_rate: f32,
    ) -> f32 {
        self.play(controls, state, outputs, sample_rate)[0]
    }
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
