use parking_lot::Mutex;
use std::{
    any::Any,
    f64::consts::PI,
    ops::{Index, IndexMut},
    sync::Arc,
};

pub const TAU: f64 = 2.0 * PI;
pub type Real = f64;
pub type Tag = usize;

/// Synth modules must implement the Signal trait. In fact one could define a
/// synth module as a struct that implements `Signal`. `as_any_mut` is necessary
/// so that modules can be downcast in order to access their specific fields.
pub trait Signal: Any {
    /// This method has the same trivial implementation for all implentors of
    /// the trait. We need it to downcast trait objects to their underlying
    /// type.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Responsible for updating the `phase` and returning the next signal
    /// value, i.e. `amplitude`.
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real;
    /// Synth modules must have a tag (name) to serve as their key in the rack.
    fn tag(&self) -> Tag;
    fn modify_tag(&mut self, f: fn(Tag) -> Tag);
    fn out(&self) -> Real;
}

/// Since `as_any_mut()` usually has the same implementation for any `Signal`
/// we provide this macro for coneniene.
#[macro_export]
macro_rules! as_any_mut {
   () => {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    };
}

/// For `Signal`s that have a `tag` field implement `as_any_mut()` and `tag()`.
#[macro_export]
macro_rules! std_signal {
    () => {
        as_any_mut!();
        fn tag(&self) -> Tag {
            self.tag
        }
        fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
            self.tag = f(self.tag);
        }
        fn out(&self) -> Real {
            self.out
        }
    };
}

#[derive(Copy, Clone)]
pub struct IdGen {
    id: usize,
}

impl IdGen {
    pub fn new() -> Self {
        Self { id: 0 }
    }

    pub fn id(&mut self) -> usize {
        let id = self.id;
        self.id += 1;
        id
    }
}

/// Types that implement the gate type can be turned on and off from within a
///`Rack`, e.g. envelope generators.
pub trait Gate {
    fn gate_on(rack: &Rack, n: Tag);
    fn gate_off(rack: &Rack, n: Tag);
}

/// If your module has `on` and `off` methods you can use this macro to generate
/// the `Gate` trait.
#[macro_export]
macro_rules! gate {
    ($t:ty) => {
        impl Gate for $t {
            fn gate_on(rack: &Rack, n: Tag) {
                if let Some(v) = rack.0[n].lock().as_any_mut().downcast_mut::<Self>() {
                    v.on();
                }
            }

            fn gate_off(rack: &Rack, n: Tag) {
                if let Some(v) = rack.0[n].lock().as_any_mut().downcast_mut::<Self>() {
                    v.off();
                }
            }
        }
    };
}

/// Signals typically need to decalare that they are `Send` so that they are
/// thread safe.
pub type Sig = dyn Signal + Send;
pub type ArcMutex<T> = Arc<Mutex<T>>;

/// Convenience function for `Arc<Mutex<...>`.
pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

impl<T> Signal for ArcMutex<T>
where
    T: Signal,
{
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.lock().signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().tag()
    }

    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.lock().modify_tag(f);
    }

    fn out(&self) -> Real {
        self.lock().out()
    }
}

impl Signal for ArcMutex<dyn Signal + Send> {
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.lock().signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().tag()
    }

    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.lock().modify_tag(f);
    }

    fn out(&self) -> Real {
        self.lock().out()
    }
}

pub trait Builder {
    fn build(&mut self) -> Self
    where
        Self: Sized + Clone,
    {
        self.clone()
    }

    fn wrap(&mut self) -> ArcMutex<Self>
    where
        Self: Sized + Clone,
    {
        arc(self.clone())
    }

    fn rack(&mut self, rack: &mut Rack) -> ArcMutex<Self>
    where
        Self: Signal + Send + Sized + Clone,
    {
        let result = arc(self.clone());
        rack.append(result.clone());
        result
    }

    fn rack_insert(&mut self, at: Tag, rack: &mut Rack) -> ArcMutex<Self>
    where
        Self: Signal + Send + Sized + Clone,
    {
        let result = arc(self.clone());
        rack.insert(at, result.clone());
        result
    }
}

/// Inputs to synth modules can either be constant (`Fix`) or a control voltage
/// from another synth module (`Cv`).
#[derive(Copy, Clone, Debug)]
pub enum In {
    Cv(Tag),
    Fix(Real),
}

impl In {
    /// Get the signal value. Look it up in the rack if it is `Cv`.
    pub fn val(rack: &Rack, inp: In) -> Real {
        match inp {
            In::Fix(x) => x,
            In::Cv(n) => rack.output(n),
        }
    }
}

impl From<Real> for In {
    fn from(x: Real) -> Self {
        In::Fix(x)
    }
}

impl From<Tag> for In {
    fn from(t: Tag) -> Self {
        In::Cv(t)
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

/// Connect the `source` module to the `field` input of the `dest` module.
pub fn connect<T, U>(source: &T, dest: &mut U, field: &'static str)
where
    T: Signal,
    U: Index<&'static str, Output = In> + IndexMut<&'static str>,
{
    dest[field] = source.tag().into();
}

/// SynthModules for the rack will have both a module (i.e an implementor of
/// `Signal`) and will store their current signal value as `output`
#[derive(Clone)]
pub struct SynthModule {
    pub module: ArcMutex<Sig>,
    pub output: Real,
}

impl SynthModule {
    pub fn new(signal: ArcMutex<Sig>) -> Self {
        Self {
            module: signal,
            output: 0.0,
        }
    }
}

impl Signal for SynthModule {
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.module.signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.module.tag()
    }

    fn modify_tag(&mut self, f: fn(Tag) -> Tag) {
        self.module.modify_tag(f);
    }

    fn out(&self) -> Real {
        self.module.out()
    }
}

#[derive(Clone)]
pub struct Rack(pub Vec<ArcMutex<Sig>>);

impl Rack {
    /// Create a `Rack` object whose order is set to the order of the `Signal`s
    /// in the input `ws`.
    pub fn new() -> Self {
        Rack(vec![])
    }

    pub fn modules(&mut self, ms: Vec<ArcMutex<Sig>>) -> &mut Self {
        self.0 = ms;
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &ArcMutex<Sig>> {
        self.0.iter()
    }

    /// Convenience function get the `Tag` of the final node in the `Rack`.
    pub fn out_tag(&self) -> Tag {
        self.0.len() - 1
    }

    /// Get the `output` of a `Node`.
    pub fn output(&self, n: Tag) -> Real {
        self.0[n].out()
    }

    /// Add a `Node` (synth module) to the `Rack` and set it's order to be last.
    pub fn append(&mut self, sig: ArcMutex<Sig>) {
        self.0.push(sig);
    }

    /// Add a `SynthModule` to the `Rack` at the position `before` was.
    pub fn insert(&mut self, at: Tag, sig: ArcMutex<Sig>) {
        for i in at..self.0.len() {
            self.0[i].modify_tag(|t| t + 1);
        }
        self.0.insert(at, sig);
    }

    /// A `Rack` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        for sm in self.iter() {
            sm.lock().signal(self, sample_rate);
        }
        self.0[self.out_tag()].out()
    }
}

impl Builder for Rack {}

/// Use to connect subracks to the main rack. Simply store the value of the
/// input node from the main rack as a link module, which will be the first
/// module in the subrack.
#[derive(Clone)]
pub struct Link {
    tag: Tag,
    value: In,
    out: Real,
}

impl Link {
    pub fn new(id_gen: &mut IdGen) -> Self {
        Self {
            tag: id_gen.id(),
            value: 0.into(),
            out: 0.0,
        }
    }

    pub fn value<T: Into<In>>(&mut self, arg: T) -> &mut Self {
        self.value = arg.into();
        self
    }
}

impl Builder for Link {}

impl Signal for Link {
    std_signal!();
    fn signal(&mut self, rack: &Rack, _sample_rate: Real) -> Real {
        self.out = In::val(rack, self.value);
        self.out
    }
}

impl Index<&str> for Link {
    type Output = In;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "value" => &self.value,
            _ => panic!("Link does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Link {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "value" => &mut self.value,
            _ => panic!("Link does not have a field named:  {}", index),
        }
    }
}
