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
// pub type Tag = &'static str;
pub type Tag = Uuid;

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
    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real;
    /// Synth modules must have a tag (name) to serve as their key in the graph.
    fn tag(&self) -> Tag;
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
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, graph: &Graph, sample_rate: Real) -> Real {
        self.lock().unwrap().signal(graph, sample_rate)
    }

    fn tag(&self) -> Tag {
        self.lock().unwrap().tag()
    }
}

/// Inputs to synth modules can either be constant (`Fix`) or a control voltage
/// from another synth module (`Cv`).
#[derive(Copy, Clone)]
pub enum In {
    Cv(Tag),
    Fix(Real),
}

impl In {
    /// Get the signal value. Look it up in the graph if it is `Cv`.
    pub fn val(graph: &Graph, inp: In) -> Real {
        match inp {
            In::Fix(x) => x,
            In::Cv(n) => graph.output(n),
        }
    }
}

/// Create a modulateable input.
pub fn cv(n: Tag) -> In {
    In::Cv(n)
}

/// Create a constant input.
pub fn fix(x: Real) -> In {
    In::Fix(x)
}

/// Nodes for the graph will have both a synth module (i.e an implentor of
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

type GraphMap = HashMap<Tag, Node>;

/// A `Graph` is basically a `HashMap` of nodes to be visited in the specified order.
#[derive(Clone)]
pub struct Graph {
    pub nodes: GraphMap,
    pub order: Vec<Tag>,
}

impl Graph {
    /// Create a `Graph` object whose order is set to the order of the `Signal`s
    /// in the input `ws`.
    pub fn new(ws: Vec<ArcMutex<Sig>>) -> Self {
        let mut nodes: GraphMap = HashMap::new();
        let mut order: Vec<Tag> = Vec::new();
        for s in ws {
            let t = s.lock().unwrap().tag();
            nodes.insert(t, Node::new(s));
            order.push(t)
        }
        Graph { nodes, order }
    }

    /// Convenience function get the `Tag` of the final node in the `Graph`.
    pub fn out_tag(&self) -> Tag {
        let n = self.nodes.len() - 1;
        self.order[n]
    }

    /// Get the `output` of a `Node`.
    pub fn output(&self, n: Tag) -> Real {
        self.nodes[&n].output
    }

    /// Insert a sub-graph into the graph before node `loc`.
    pub fn insert(&mut self, graph: Graph, loc: usize) {
        let n = graph.nodes.len() + self.nodes.len();
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
        self.nodes.extend(graph.nodes);
    }

    /// A `Graph` generates a signal by travesing the list of modules and
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

//TODO: return Result struct indicating success or failure
pub trait Set<'a>: IndexMut<&'a str> {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real);
}

/// Use to connect subgraphs to the main graph. Simply store the value of the
/// input node from the main graph as a connect node, which will be the first
/// node in the subgraph.
pub struct Connect {
    pub tag: Tag,
    pub value: Real,
}

impl Connect {
    pub fn new() -> Self {
        Self { tag: mk_tag(), value: 0.0 }
    }

    pub fn wrapped() -> ArcMutex<Self> {
        arc(Self::new())
    }
}

impl Signal for Connect {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn signal(&mut self, _graph: &Graph, _sample_rate: Real) -> Real {
        self.value
    }

    fn tag(&self) -> Tag {
        self.tag
    }
}

impl Index<&str> for Connect {
    type Output = Real;

    fn index(&self, index: &str) -> &Self::Output {
        match index {
            "value" => &self.value,
            _ => panic!("Connect does not have a field named:  {}", index),
        }
    }
}

impl IndexMut<&str> for Connect {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        match index {
            "value" => &mut self.value,
            _ => panic!("MidiPitch does not have a field named:  {}", index),
        }
    }
}

impl<'a> Set<'a> for Connect {
    fn set(graph: &Graph, n: Tag, field: &str, value: Real) {
        if let Some(v) = graph.nodes[&n]
            .module
            .lock()
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Self>()
        {
            v[field] = value;
        }
    }
}
