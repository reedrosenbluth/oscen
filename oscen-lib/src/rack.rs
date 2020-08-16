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
pub struct Controls(pub [[In; MAX_CONTROLS]; MAX_MODULES]);
pub struct Outputs(pub [[Real; MAX_OUTPUTS]; MAX_MODULES]);

impl Controls {
    pub fn controls(&self, tag: Tag) -> &[In] {
        self.0[tag].as_ref()
    }
    pub fn controls_mut(&mut self, tag: Tag) -> &mut [In] {
        self.0[tag].as_mut()
    }
}

impl Outputs {
    pub fn outputs(&self, tag: Tag) -> &[Real] {
        self.0[tag].as_ref()
    }
    pub fn outputs_mut(&mut self, tag: Tag) -> &mut [Real] {
        self.0[tag].as_mut()
    }
    pub fn value(&self, inp: In) -> Real {
        match inp {
            In::Fix(p) => p,
            In::Cv(n, i) => self.0[n][i],
        }
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
pub struct Rack(pub Vec<Box<dyn Signal + Send + Sync>>);

impl Rack {
    pub fn num_modules(&self) -> usize {
        self.0.len()
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
