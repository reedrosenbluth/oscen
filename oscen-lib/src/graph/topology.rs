use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;

/// Error type for topology operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError<K: Debug + Clone> {
    /// Cycle detected in the graph
    CycleDetected { path: Vec<K> },
}

impl<K: Debug + Clone> std::fmt::Display for TopologyError<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopologyError::CycleDetected { path } => {
                write!(f, "Cycle detected: ")?;
                for (i, node) in path.iter().enumerate() {
                    if i > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{:?}", node)?;
                }
                Ok(())
            }
        }
    }
}

impl<K: Debug + Clone> std::error::Error for TopologyError<K> {}

/// Generic topological sort using DFS
///
/// # Arguments
/// * `nodes` - Iterator over all nodes to sort
/// * `get_dependencies` - Function that returns the dependencies (predecessors) for a node
/// * `allows_feedback` - Function that returns true if a node can be part of a feedback loop
///
/// # Returns
/// A vector of nodes in topological order (dependencies before dependents),
/// or an error if a cycle is detected without a feedback-allowing node.
///
/// # Algorithm
/// 1. For nodes that allow feedback, temporarily remove their outgoing edges
/// 2. Perform DFS-based topological sort with cycle detection
/// 3. Verify any remaining cycles contain at least one feedback-allowing node
pub fn topological_sort<K>(
    nodes: impl IntoIterator<Item = K>,
    get_dependencies: impl Fn(&K) -> Vec<K>,
    allows_feedback: impl Fn(&K) -> bool,
) -> Result<Vec<K>, TopologyError<K>>
where
    K: Hash + Eq + Clone + Debug,
{
    let nodes: Vec<K> = nodes.into_iter().collect();

    // Build adjacency map from dependencies
    let mut adjacency: HashMap<K, Vec<K>> = HashMap::new();
    for node in &nodes {
        adjacency.insert(node.clone(), Vec::new());
    }

    for node in &nodes {
        let deps = get_dependencies(node);
        for dep in deps {
            // Edge from dep -> node (dep must be processed before node)
            adjacency.entry(dep.clone())
                .or_insert_with(Vec::new)
                .push(node.clone());
        }
    }

    // Identify feedback-allowing nodes (like delay nodes)
    let feedback_nodes: HashSet<K> = nodes
        .iter()
        .filter(|n| allows_feedback(n))
        .cloned()
        .collect();

    // For sorting, remove outgoing edges from feedback nodes to break cycles
    let mut sort_adjacency = adjacency.clone();
    for feedback_node in &feedback_nodes {
        sort_adjacency.insert(feedback_node.clone(), Vec::new());
    }

    // Perform DFS-based topological sort
    let mut sorted = Vec::with_capacity(nodes.len());
    let mut visited = HashSet::new();
    let mut recursion_stack = HashSet::new();

    fn visit<K>(
        node: K,
        adjacency: &HashMap<K, Vec<K>>,
        visited: &mut HashSet<K>,
        recursion_stack: &mut HashSet<K>,
        sorted: &mut Vec<K>,
    ) -> Result<(), TopologyError<K>>
    where
        K: Hash + Eq + Clone + Debug,
    {
        if recursion_stack.contains(&node) {
            // Cycle detected - build path for error message
            return Err(TopologyError::CycleDetected {
                path: vec![node.clone()],
            });
        }

        if visited.contains(&node) {
            return Ok(());
        }

        visited.insert(node.clone());
        recursion_stack.insert(node.clone());

        if let Some(neighbors) = adjacency.get(&node) {
            for neighbor in neighbors {
                visit(neighbor.clone(), adjacency, visited, recursion_stack, sorted)?;
            }
        }

        recursion_stack.remove(&node);
        sorted.push(node);

        Ok(())
    }

    for node in &nodes {
        if !visited.contains(node) {
            visit(
                node.clone(),
                &sort_adjacency,
                &mut visited,
                &mut recursion_stack,
                &mut sorted,
            )?;
        }
    }

    // Reverse to get dependency order (dependencies first)
    sorted.reverse();

    // Verify that any cycles in the original graph contain feedback nodes
    verify_cycles_have_feedback(&nodes, &adjacency, &feedback_nodes)?;

    Ok(sorted)
}

/// Verify that all cycles contain at least one feedback-allowing node
fn verify_cycles_have_feedback<K>(
    nodes: &[K],
    adjacency: &HashMap<K, Vec<K>>,
    feedback_nodes: &HashSet<K>,
) -> Result<(), TopologyError<K>>
where
    K: Hash + Eq + Clone + Debug,
{
    let mut visited = HashSet::new();
    let mut recursion_stack = HashSet::new();
    let mut path = Vec::new();

    fn find_cycle<K>(
        node: K,
        adjacency: &HashMap<K, Vec<K>>,
        visited: &mut HashSet<K>,
        recursion_stack: &mut HashSet<K>,
        path: &mut Vec<K>,
        feedback_nodes: &HashSet<K>,
    ) -> Result<(), TopologyError<K>>
    where
        K: Hash + Eq + Clone + Debug,
    {
        visited.insert(node.clone());
        recursion_stack.insert(node.clone());
        path.push(node.clone());

        if let Some(neighbors) = adjacency.get(&node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    find_cycle(
                        neighbor.clone(),
                        adjacency,
                        visited,
                        recursion_stack,
                        path,
                        feedback_nodes,
                    )?;
                } else if recursion_stack.contains(neighbor) {
                    // Found a cycle - extract it from the path
                    let cycle_start = path.iter().position(|n| n == neighbor).unwrap();
                    let cycle_nodes: Vec<K> = path[cycle_start..].to_vec();

                    // Check if any node in the cycle allows feedback
                    let has_feedback = cycle_nodes.iter().any(|n| feedback_nodes.contains(n));

                    if !has_feedback {
                        return Err(TopologyError::CycleDetected { path: cycle_nodes });
                    }
                }
            }
        }

        recursion_stack.remove(&node);
        path.pop();
        Ok(())
    }

    for node in nodes {
        if !visited.contains(node) {
            find_cycle(
                node.clone(),
                adjacency,
                &mut visited,
                &mut recursion_stack,
                &mut path,
                feedback_nodes,
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_chain() {
        // a -> b -> c
        let nodes = vec!["a", "b", "c"];
        let deps = |node: &&str| -> Vec<&str> {
            match *node {
                "b" => vec!["a"],
                "c" => vec!["b"],
                _ => vec![],
            }
        };

        let sorted = topological_sort(nodes, deps, |_| false).unwrap();
        assert_eq!(sorted, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_diamond() {
        // a -> b -> d
        // a -> c -> d
        let nodes = vec!["a", "b", "c", "d"];
        let deps = |node: &&str| -> Vec<&str> {
            match *node {
                "b" => vec!["a"],
                "c" => vec!["a"],
                "d" => vec!["b", "c"],
                _ => vec![],
            }
        };

        let sorted = topological_sort(nodes, deps, |_| false).unwrap();
        assert_eq!(sorted[0], "a");
        assert_eq!(sorted[3], "d");
        // b and c can be in either order
    }

    #[test]
    fn test_cycle_detection() {
        // a -> b -> a (cycle)
        let nodes = vec!["a", "b"];
        let deps = |node: &&str| -> Vec<&str> {
            match *node {
                "a" => vec!["b"],
                "b" => vec!["a"],
                _ => vec![],
            }
        };

        let result = topological_sort(nodes, deps, |_| false);
        assert!(result.is_err());
        match result {
            Err(TopologyError::CycleDetected { .. }) => (),
            _ => panic!("Expected cycle error"),
        }
    }

    #[test]
    fn test_feedback_breaks_cycle() {
        // a -> b -> a (cycle, but a allows feedback)
        let nodes = vec!["a", "b"];
        let deps = |node: &&str| -> Vec<&str> {
            match *node {
                "a" => vec!["b"],
                "b" => vec!["a"],
                _ => vec![],
            }
        };
        let allows_feedback = |node: &&str| *node == "a";

        let sorted = topological_sort(nodes, deps, allows_feedback).unwrap();
        // Should succeed because 'a' allows feedback
        assert_eq!(sorted.len(), 2);
    }
}
