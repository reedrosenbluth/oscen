use approx::relative_eq;
use std::{
    any::Any,
    collections::HashMap,
    f64::consts::PI,
    ops::{Index, IndexMut},
    sync::{Arc, Mutex},
};

use uuid::Uuid;

pub const TAU: f64 = 2.0 * PI;
pub type Real = f64;
pub type Tag = Uuid;

/// Generate a unique tag for a synth module.
pub fn mk_tag() -> Tag {
    Uuid::new_v4()
}

/// Synth modules must implement the Signal trait. `as_any_mut` is necessary
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
}

#[macro_export]
macro_rules! as_any_mut {
   () => {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    };
}

#[macro_export]
macro_rules! std_signal {
    () => {
        as_any_mut!();
        fn tag(&self) -> Tag {
            self.tag
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
}

impl Signal for ArcMutex<dyn Signal + Send> {
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.lock().unwrap().signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().unwrap().tag()
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

    /// Convenient way to create a constant `In` of zero.
    pub fn zero() -> In {
        Self::Fix(0.0)
    }

    /// Convenient way to create a constant `In` of one.
    pub fn one() -> In {
        Self::Fix(1.0)
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

impl Default for In {
    fn default() -> Self {
        Self::Fix(0.0)
    }
}

/// Connect the `source` node to the `field` input of the `dest` node.
pub fn connect<T, U>(source: &T, dest: &mut U, field: &'static str)
where
    T: Signal,
    U: Index<&'static str, Output = In> + IndexMut<&'static str>,
{
    dest[field] = source.tag().into();
}

/// Nodes for the rack will have both a synth module (i.e an implentor of
/// `Signal`) and will store their current signal value as `output`
#[derive(Clone)]
pub struct Node {
    pub module: ArcMutex<Sig>,
    pub output: Real,
}

impl Node {
    fn new(signal: ArcMutex<Sig>) -> Self {
        Self {
            module: signal,
            output: 0.0,
        }
    }
}

impl Signal for Node {
    as_any_mut!();
    fn signal(&mut self, rack: &Rack, sample_rate: Real) -> Real {
        self.module.signal(rack, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.module.tag()
    }
}

/// A `Rack` is basically a `HashMap` of nodes to be visited in the specified order.
#[derive(Clone)]
pub struct Rack {
    pub nodes: HashMap<Tag, Node>,
    pub order: Vec<Tag>,
}

impl Rack {
    /// Create a `Rack` object whose order is set to the order of the `Signal`s
    /// in the input `ws`.
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut nodes: HashMap<Tag, Node> = HashMap::new();
        let mut order: Vec<Tag> = Vec::new();
        for s in ws {
            let t = s.lock().unwrap().tag();
            nodes.insert(t, Node::new(s));
            order.push(t)
        }
        Rack { nodes, order }
    }

    /// Convenience function get the `Tag` of the final node in the `Rack`.
    pub fn out_tag(&self) -> Tag {
        let n = self.nodes.len() - 1;
        self.order[n]
    }

    /// Get the `output` of a `Node`.
    pub fn output(&self, n: Tag) -> Real {
        self.nodes[&n].output
    }

    pub fn append(&mut self, sig: ArcMutex<Sig>) {
        let tag = sig.tag();
        let node = Node::new(sig);
        self.nodes.insert(tag, node);
        self.order.push(tag);
    }

    /// Insert a sub-rack into the rack before node `loc`.
    pub fn insert(&mut self, rack: Rack, loc: usize) {
        let n = rack.nodes.len() + self.nodes.len();
        let mut new_order: Vec<Tag> = Vec::with_capacity(n);
        for i in 0..loc {
            new_order.push(self.order[i])
        }
        for i in 0..rack.order.len() {
            new_order.push(rack.order[i])
        }
        for i in loc..self.nodes.len() {
            new_order.push(self.order[i])
        }
        self.order = new_order;
        self.nodes.extend(rack.nodes);
    }

    /// A `Rack` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        let mut outs: Vec<Real> = Vec::new();
        for o in self.order.iter() {
            outs.push(
                self.nodes[o]
                    .module
                    .lock()
                    .unwrap()
                    .signal(&self, sample_rate),
            );
        }
        for (i, o) in self.order.iter().enumerate() {
            self.nodes.get_mut(o).unwrap().output = outs[i];
        }
        self.nodes[&self.out_tag()].output
    }
}
/// Use to connect subracks to the main rack. Simply store the value of the
/// input node from the main rack as a connect node, which will be the first
/// node in the subrack.
#[derive(Clone)]
pub struct Link {
    pub tag: Tag,
    pub value: In,
}

impl Link {
    pub fn new() -> Self {
        Self {
            tag: mk_tag(),
            value: In::zero(),
        }
    }
}

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


/// Given f(0) = low, f(1/2) = mid, and f(1) = high, let f(x) = a + b*exp(cs).
/// Fit a, b, and c so to match the above. If mid < 1/2(high + low) then f is
/// convex, if equal f is linear, if great then f is concave.
pub fn exp_interp(low: Real, mid: Real, high: Real, x: Real) -> Real {
    if relative_eq!(2.0 * mid, high + low) {
        return low + (high - low) * x;
    }
    let b = (mid - low) * (mid - low) / (high - 2.0 * mid + low);
    let a = low - b;
    let c = 2.0 * ((high - mid) / (mid - low)).ln();
    a + b * (c * x).exp()
}
