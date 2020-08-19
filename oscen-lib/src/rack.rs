use std::ops::{Index, IndexMut};

/// Unique identifier for each Synth Module.
pub type Tag = usize;
pub type Real = f32;
pub type SignalFn = fn(Real, Real) -> Real;

pub const MAX_CONTROLS: usize = 32;
pub const MAX_OUTPUTS: usize = 32;
pub const MAX_MODULES: usize = 1024;

/// Inputs to Synth Modules can either be constant (`Fix`) or a control voltage
/// from another synth module (`Cv`). The tag is the unique id of the module and
/// the usize is the index of the output vector.
#[derive(Copy, Clone, Debug)]
pub enum In {
    Cv(Tag, usize),
    Fix(Real),
}

impl From<Real> for In {
    fn from(x: Real) -> Self {
        In::Fix(x)
    }
}

impl From<i32> for In {
    fn from(i: i32) -> Self {
        In::Fix(i as Real)
    }
}

impl Default for In {
    fn default() -> Self {
        Self::Fix(0.0)
    }
}

#[derive(Copy, Clone)]
pub enum Control {
    V(In),
    B(bool),
    I(usize),
}

impl From<Real> for Control {
    fn from(x: Real) -> Self {
       Control::V(In::Fix(x)) 
    }
}

impl From<usize> for Control {
    fn from(u: usize) -> Self {
        Control::V(In::Fix(u as Real))
    }
}

impl From<bool> for Control {
    fn from(b: bool) -> Self {
        Control::B(b)
    }
}

#[derive(Copy, Clone)]
pub struct Controls([[Control; MAX_CONTROLS]; MAX_MODULES]);

#[derive(Copy, Clone)]
pub struct Outputs([[Real; MAX_OUTPUTS]; MAX_MODULES]);

impl Controls {
    pub fn new() -> Self {
        Controls([[0.into(); MAX_CONTROLS]; MAX_MODULES])
    }
    pub fn controls(&self, tag: Tag) -> &[Control] {
        self.0[tag].as_ref()
    }
    pub fn controls_mut(&mut self, tag: Tag) -> &mut [Control] {
        self.0[tag].as_mut()
    }
}

impl Index<(Tag, usize)> for Controls {
    type Output = Control;
    fn index(&self, index: (Tag, usize)) -> &Self::Output {
        &self.controls(index.0)[index.1]
    }
}

impl IndexMut<(Tag, usize)> for Controls {
    fn index_mut(&mut self, index: (Tag, usize)) -> &mut Self::Output {
        &mut self.controls_mut(index.0)[index.1]
    }
}

impl Outputs {
    pub fn new() -> Self {
        Outputs([[0.0; MAX_OUTPUTS]; MAX_MODULES])
    }
    pub fn outputs(&self, tag: Tag) -> &[Real] {
        self.0[tag].as_ref()
    }
    pub fn outputs_mut(&mut self, tag: Tag) -> &mut [Real] {
        self.0[tag].as_mut()
    }
    pub fn value(&self, ctrl: Control) -> Option<Real> {
        match ctrl {
            Control::V(In::Fix(p)) => Some(p),
            Control::V(In::Cv(n, i)) => Some(self.0[n][i]),
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

impl Index<(Tag, usize)> for Outputs {
    type Output = Real;
    fn index(&self, index: (Tag, usize)) -> &Self::Output {
        &self.outputs(index.0)[index.1]
    }
}

impl IndexMut<(Tag, usize)> for Outputs {
    fn index_mut(&mut self, index: (Tag, usize)) -> &mut Self::Output {
        &mut self.outputs_mut(index.0)[index.1]
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
    fn signal(&mut self, controls: &Controls, outputs: &mut Outputs, sample_rate: Real);
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
pub struct Rack(Vec<Box<dyn Signal + Send + Sync>>);

impl Rack {
    pub fn new() -> Self {
        Rack(vec![])
    }
    pub fn num_modules(&self) -> usize {
        self.0.len()
    }
    pub fn push(&mut self, module: Box<dyn Signal + Send + Sync>) {
        self.0.push(module);
    }
    /// Call the `signal` function for each module in turn returning the vector
    /// of outpts in the last module.
    pub fn play(
        &mut self,
        controls: &Controls,
        outputs: &mut Outputs,
        sample_rate: Real,
    ) -> [Real; MAX_OUTPUTS] {
        let n = self.0.len() - 1;
        for module in self.0.iter_mut() {
            module.signal(controls, outputs, sample_rate);
        }
        outputs.0[n]
    }
    /// Like play but only returns the sample in `outputs[0].
    pub fn mono(&mut self, controls: &Controls, outpus: &mut Outputs, sample_rate: Real) -> Real {
        self.play(controls, outpus, sample_rate)[0]
    }
}
