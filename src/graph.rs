use std::{
    any::Any,
    f64::consts::PI,
    sync::{Arc, Mutex},
};

pub const TAU: f64 = 2.0 * PI;
pub type Real = f64;
pub type Tag = usize;

/// Synth modules must implement the Signal trait. `as_any_mut` is necessary
/// so that modules can be downcast in order to access their specific fields.
pub trait Signal: Any {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Responsible for updating the `phase` and returning the next signal
    /// value, i.e. `amplitude`.
    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real;
}

/// Signals typically need to decalare that they are `Send` so that they are
/// thread safe.
pub type Sig = dyn Signal + Send;
pub type ArcMutex<T> = Arc<Mutex<T>>;

/// Convenience function for `Arc<Mutex<...>`.
pub fn arc<T>(x: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(x))
}

/// Inputs to synth modules can either be constant (`Fixed`) or modulated by
/// another signal (`Var`).
#[derive(Copy, Clone)]
pub enum In {
    Var(Tag),
    Fixed(Real),
}

impl In {
    /// Get the signal value. Look it up in the graph if it is `Var`.
    pub fn val(graph: &Graph, inp: In) -> Real {
        match inp {
            In::Fixed(x) => x,
            In::Var(n) => graph.output(n),
        }
    }
}

/// Create a modulateable input.
pub fn var(n: Tag) -> In {
    In::Var(n)
}

/// Create a constant input.
pub fn fix(x: Real) -> In {
    In::Fixed(x)
}

/// Nodes for the graph will have both a synth module (i.e an implentor of
/// `Signal`) and will store there previous signal value as `output`
pub struct Node {
    pub module: ArcMutex<Sig>,
    pub output: Real,
}

impl Node {
    fn new(signal: ArcMutex<Sig>) -> Self {
        Node {
            module: signal,
            output: 0.0,
        }
    }
}

/// A `Graph` is just a vector of nodes to be visited in order.
pub struct Graph(pub Vec<Node>);

impl Graph {
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut ns: Vec<Node> = Vec::new();
        for s in ws {
            ns.push(Node::new(s));
        }
        Graph(ns)
    }

    pub fn output(&self, n: Tag) -> Real {
        self.0[n].output
    }
    /// A `Graph` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        let mut outs: Vec<Real> = Vec::new();
        for node in self.0.iter() {
            outs.push(node.module.lock().unwrap().signal(&self, sample_rate));
        }
        for (i, node) in self.0.iter_mut().enumerate() {
            node.output = outs[i];
        }
        self.0[self.0.len() - 1].output
    }
}
