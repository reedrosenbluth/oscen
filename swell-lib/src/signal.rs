use std::{
    any::Any,
    collections::HashMap,
    f64::consts::PI,
    ops::{Index, IndexMut},
    sync::{Arc, Mutex}, fmt::Debug,
};

pub const TAU: f64 = 2.0 * PI;
pub type Real = f64;
pub type Tag = u32;

/// Generate a unique tag for a synth module.
pub fn mk_tag() -> Tag {
    0
}

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
    fn set_tag(&mut self, tag: Tag);
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
        fn set_tag(&mut self, tag: Tag) {
            self.tag = tag;
        }
    };
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
                if let Some(v) = rack.modules[&n]
                    .module
                    .lock()
                    .unwrap()
                    .as_any_mut()
                    .downcast_mut::<Self>()
                {
                    v.on();
                }
            }

            fn gate_off(rack: &Rack, n: Tag) {
                if let Some(v) = rack.modules[&n]
                    .module
                    .lock()
                    .unwrap()
                    .as_any_mut()
                    .downcast_mut::<Self>()
                {
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
        self.lock().unwrap().signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().unwrap().tag()
    }

    fn set_tag(&mut self, tag: Tag) {
        self.lock().unwrap().set_tag(tag);
    }
}

impl Signal for ArcMutex<dyn Signal + Send> {
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.lock().unwrap().signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().unwrap().tag()
    }

    fn set_tag(&mut self, tag: Tag) {
        self.lock().unwrap().set_tag(tag);
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

    fn rack_pre(&mut self, rack: &mut Rack) -> ArcMutex<Self>
    where
        Self: Signal + Send + Sized + Clone,
    {
        let result = arc(self.clone());
        rack.preppend(result.clone());
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
    fn new(signal: ArcMutex<Sig>) -> Self {
        Self {
            module: signal,
            output: 0.0,
        }
    }
}

impl Debug for SynthModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SynthModule")
         .field("tag", &self.module.lock().unwrap().tag())
         .field("output", &self.output)
         .finish()
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

    fn set_tag(&mut self, tag: Tag) {
        self.module.set_tag(tag);
    }
}

/// A `Rack` is basically a `HashMap` of synth modules to be visited in the specified order.
#[derive(Debug, Clone)]
pub struct Rack {
    pub modules: HashMap<Tag, SynthModule>,
    pub order: Vec<Tag>,
}

impl Rack {
    /// Create a `Rack` object whose order is set to the order of the `Signal`s
    /// in the input `ws`.
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut nodes: HashMap<Tag, SynthModule> = HashMap::new();
        let mut order: Vec<Tag> = Vec::new();
        for s in ws {
            let t = s.lock().unwrap().tag();
            nodes.insert(t, SynthModule::new(s));
            order.push(t)
        }
        Rack {
            modules: nodes,
            order,
        }
    }

    /// Convert a rack into an `Iter` - note: we don't need an `iter_mut` since
    /// we will mostly mutating a `Node` via it's `Mutex`.
    pub fn iter<'a>(&'a self) -> Iter<'a> {
        Iter {
            rack: self,
            index: 0,
        }
    }

    /// Convenience function get the `Tag` of the final node in the `Rack`.
    pub fn out_tag(&self) -> Tag {
        let n = self.modules.len() - 1;
        self.order[n]
    }

    /// Get the `output` of a `Node`.
    pub fn output(&self, n: Tag) -> Real {
        self.modules
            .get(&n)
            .expect("Function output could not find tag")
            .output
    }

    /// Add a `Node` (synth module) to the `Rack` and set it's order to be last.
    pub fn append(&mut self, sig: ArcMutex<Sig>) {
        let tag = sig.tag();
        let node = SynthModule::new(sig);
        self.modules.insert(tag, node);
        self.order.push(tag);
    }

    /// Add a `SynthModule` to the `Rack` and set it's order to be first`.
    pub fn preppend(&mut self, sig: ArcMutex<Sig>) {
        let tag = sig.tag();
        let node = SynthModule::new(sig);
        self.modules.insert(tag, node);
        self.order.insert(0, tag);
    }

    /// Add a `SynthModule` to the `Rack` at the position `before` was.
    pub fn before(&mut self, before: Tag, sig: ArcMutex<Sig>) {
        if let Some(pos) = self.order.iter().position(|&x| x == before) {
            let tag = sig.tag();
            let node = SynthModule::new(sig);
            self.modules.insert(tag, node);
            self.order.insert(pos, tag);
        } else {
            panic!("rack does not contain {} tag", before);
        }
    }

    /// Insert a sub-rack into the rack before node `loc`.
    pub fn insert(&mut self, rack: Rack, loc: usize) {
        let n = rack.modules.len() + self.modules.len();
        let mut new_order: Vec<Tag> = Vec::with_capacity(n);
        for i in 0..loc {
            new_order.push(self.order[i])
        }
        for i in 0..rack.order.len() {
            new_order.push(rack.order[i])
        }
        for i in loc..self.modules.len() {
            new_order.push(self.order[i])
        }
        self.order = new_order;
        self.modules.extend(rack.modules);
    }

    /// A `Rack` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        let mut outs: Vec<Real> = Vec::new();
        for node in self.iter() {
            println!("Node: {:?}", node);
            outs.push(
                node.module
                    .lock()
                    .expect("Function rack::signal could not find tag in first loop")
                    .signal(&self, sample_rate),
            )
        }
        println!("Before Second Loop");
        for (i, o) in self.order.iter().enumerate() {
            self.modules
                .get_mut(o)
                .expect("Function rack::signal could not find tag in second loop")
                .output = outs[i];
        }
        self.modules[&self.out_tag()].output
    }
}

pub struct Iter<'a> {
    rack: &'a Rack,
    index: usize,
}

impl<'a> IntoIterator for &'a Rack {
    type Item = &'a SynthModule;
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a SynthModule;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.rack.order.len() {
            return None;
        }
        let tag = self.rack.order[self.index];
        self.index += 1;
        self.rack.modules.get(&tag)
    }
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub rack: Rack,
    pub next_tag: u32,
    pub sample_rate: Real,
}

impl Environment {
    pub fn new(sample_rate: Real) -> Self {
        Self {
            rack: Rack::new(Vec::new()),
            next_tag: 0,
            sample_rate,
        }
    }
}

pub struct State<'a, A> {
    pub run: Box<dyn 'a + Fn(Environment) -> (A, Environment)>,
}

impl<'a, A: 'a + Clone> State<'a, A> {
    pub fn pure(a: A) -> Self {
        State {
            run: Box::new(move |e: Environment| (a.clone(), e)),
        }
    }

    pub fn and_then<B, F: 'a>(self, f: F) -> State<'a, B>
    where
        F: Fn(A) -> State<'a, B>,
    {
        State {
            run: Box::new(move |e: Environment| {
                let (v, e1) = (*self.run)(e);
                let g = f(v).run;
                (*g)(e1)
            }),
        }
    }
}

pub fn get_state<'a>() -> State<'a, Environment> {
    State {
        run: Box::new(|e: Environment| (e.clone(), e)),
    }
}

pub fn put_state<'a>(e: Environment) -> State<'a, ()> {
    State {
        run: Box::new(move |_| ((), e.clone())),
    }
}

pub fn modify_state<'a, F: 'a>(f: F) -> State<'a, ()>
where
    F: Fn(Environment) -> Environment,
{
    let e = get_state();
    e.and_then(Box::new(move |x| put_state(f(x))))
}

pub fn eval_state<'a, A>(state: State<'a, A>, e: Environment) -> A {
    (state.run)(e).0
}

pub fn exec_state<'a, A>(state: State<'a, A>, e: Environment) -> Environment {
    (state.run)(e).1
}

// pub fn next_tag<'a>() -> State<'a, u32> {
//     let g = |e| {
//         let env = (get_state().run)(e).0;
//         let t = env.next_tag + 1;
//         Environment {
//             rack: env.rack.clone(),
//             next_tag: t,
//             sample_rate: env.sample_rate,
//         }
//     };
//     modify_state(Box::new(g))
//         .and_then(Box::new(|()| get_state()))
//         .and_then(|e| State::pure(e.next_tag))
// }

pub fn rack_append<'a>(module: ArcMutex<dyn Signal + Send>) -> State<'a, ()> {
    let g = move |e| {
        let env = (get_state().run)(e).0;
        let t = env.next_tag + 1;
        module.lock().unwrap().set_tag(t);
        let mut rack = env.rack.clone();
        rack.append(module.clone());
        Environment
         {
            rack,
            next_tag: t,
            sample_rate: env.sample_rate,
        }
    };
    modify_state(Box::new(g))
}

/// Use to connect subracks to the main rack. Simply store the value of the
/// input node from the main rack as a link module, which will be the first
/// module in the subrack.
#[derive(Clone)]
pub struct Link {
    tag: Tag,
    value: In,
}

impl Link {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            value: 0.into(),
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
        In::val(rack, self.value)
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
