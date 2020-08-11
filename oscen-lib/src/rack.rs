use std::sync::Arc;

/// Unique identifier for each Synth Module.
pub type Tag = usize;
pub type Real = f32;

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

/// A union type encapsulating the abstraction for types of parameters for a
/// Synth Module.
#[derive(Copy, Clone, Debug)]
pub enum Parameter {
    Bool(bool),
    Int(i32),
    UInt(usize),
    Float(f64),
}

/// Each Synth Module has associated date `ModuleData` that stores any inputs,
/// parameters, buffer (e.g. filters), and outputs.
#[derive(Debug, Clone)]
pub struct ModuleData {
    inputs: Vec<In>,
    parameters: Vec<Parameter>,
    buffer: Vec<Real>,
    outputs: Vec<Real>,
}

impl ModuleData {
    pub fn new(inputs: Vec<In>, outputs: Vec<Real>) -> Self {
        Self {
            inputs,
            parameters: vec![],
            buffer: vec![],
            outputs,
        }
    }
    pub fn parameters(&mut self, values: Vec<Parameter>) -> &mut Self {
        self.parameters = values;
        self
    }
    pub fn buffer(&mut self, values: Vec<Real>) -> &mut Self {
        self.buffer = values;
        self
    }
    pub fn build(&mut self) -> Self
    where
        Self: Clone,
    {
        self.clone()
    }
}

/// A `ModuleTable` contains all mutable data for a synth.
#[derive(Debug, Clone)]
pub struct ModuleTable {
    table: Vec<ModuleData>,
}

impl ModuleTable {
    pub fn new(table: Vec<ModuleData>) -> Self {
        Self { table }
    }
    pub fn inputs(&self, n: Tag) -> &[In] {
        self.table[n].inputs.as_slice()
    }
    pub fn inputs_mut(&mut self, n: Tag) -> &mut [In] {
        self.table[n].inputs.as_mut_slice()
    }
    pub fn parameters(&self, n: Tag) -> &[Parameter] {
        self.table[n].parameters.as_slice()
    }
    pub fn parameters_mut(&mut self, n: Tag) -> &mut [Parameter] {
        self.table[n].parameters.as_mut_slice()
    }
    pub fn buffer(&self, n: Tag) -> &[Real] {
        self.table[n].buffer.as_slice()
    }
    pub fn buffer_mut(&mut self, n: Tag) -> &mut [Real] {
        self.table[n].buffer.as_mut_slice()
    }
    pub fn outputs(&self, n: Tag) -> &[Real] {
        self.table[n].outputs.as_slice()
    }
    pub fn outputs_mut(&mut self, n: Tag) -> &mut [Real] {
        self.table[n].outputs.as_mut_slice()
    }
    pub fn value(&self, inp: In) -> Real {
        match inp {
            In::Fix(p) => p,
            In::Cv(n, i) => self.table[n].outputs[i],
        }
    }
    pub fn push(&mut self, value:ModuleData) {
        self.table.push(value);
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
    fn signal(&self, modules: &mut ModuleTable, sample_rate: Real);
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


/// A Rack is a topologically sorted `Vector` of Synth Modules. A synth is one or
/// more racks. All mutable data is stored in the `ModuleTable`.
pub struct Rack(pub Vec<Arc<dyn Signal + Send + Sync>>);

impl Rack {
    /// Call the `signal` function for each module in turn returning the vector
    /// of outpts in the last module.
    pub fn play(&self, table: &mut ModuleTable, sample_rate: Real) -> Vec<Real> {
        let n = self.0.len() - 1;
        for module in self.0.iter() {
            module.signal(table, sample_rate);
        }
        table.outputs(n).to_owned()
    }
    /// Like play but only returns the sample in `outputs[0].
    pub fn mono(&self, table: &mut ModuleTable, sample_rate: Real) -> Real {
        self.play(table, sample_rate)[0]
    }
}
