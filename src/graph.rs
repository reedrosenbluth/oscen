use std::{
    any::Any,
    f64::consts::PI,
    ops::IndexMut,
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
#[derive(Clone)]
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

/// A `Graph` is basically a vector of nodes to be visited in the specified order.
#[derive(Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub order: Vec<Tag>,
}

impl Graph {
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut nodes: Vec<Node> = Vec::new();
        let n = ws.len();
        for s in ws {
            nodes.push(Node::new(s));
        }
        let order: Vec<Tag> = (0..n).collect();
        Graph { nodes, order }
    }

    pub fn next_tag(&self) -> Tag {
        self.nodes.len()
    }

    pub fn out_tag(&self) -> Tag {
        let n = self.nodes.len() - 1;
        self.order[n]
    }

    pub fn output(&self, n: Tag) -> Real {
        self.nodes[n].output
    }

    /// Insert a sub-graph into the graph before node `loc`.
    pub fn insert(&mut self, graph: &mut Graph, loc: Tag) {
        let n = graph.nodes.len() + self.nodes.len();
        self.nodes.append(&mut graph.nodes);
        let mut new_order: Vec<Tag> = Vec::with_capacity(n);
        for i in 0..loc {
            new_order.push(self.order[i])
        }
        for i in 0..graph.order.len() {
            new_order.push(graph.order[i])
        }
        for i in loc..self.nodes.len() {
            new_order.push(self.order[i])
        }
        self.order = new_order;
    }

    /// A `Graph` generates a signal by travesing the list of modules and
    /// updating each one's output in turn. The output of the last `Node` is
    /// returned.
    pub fn signal(&mut self, sample_rate: Real) -> Real {
        let mut outs: Vec<Real> = Vec::new();
        for o in self.order.iter() {
            outs.push(
                self.nodes[*o]
                    .module
                    .lock()
                    .unwrap()
                    .signal(&self, sample_rate),
            );
        }
        for o in self.order.iter() {
            self.nodes[*o].output = outs[*o];
        }
        self.nodes[self.out_tag()].output
    }
}

pub trait Set<'a>: IndexMut<&'a str> {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real); 
}
