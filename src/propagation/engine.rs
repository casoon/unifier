//! Arc-consistency AC-3 propagation engine.
//!
//! Maintains a queue of constraints to re-evaluate whenever variable domains shrink,
//! propagating domain reductions until a fixpoint or conflict is reached.
//!
//! References:
//! - Mackworth, A. K. (1977). *Consistency in networks of relations*. Artificial Intelligence, 8(1), 99-118.
//! - Schulte, C., & Stuckey, P. J. (2008). *Efficient constraint propagation engines*. ACM TOPLAS, 31(1), 1-43.

use crate::constraint::PropagationResult;
use crate::model::domain::TrailedDomains;
use crate::propagation::graph::{ConstraintGraph, ConstraintId};
use std::collections::{HashMap, VecDeque};

/// Propagation engine executing AC-3 domain filtering across a [`ConstraintGraph`].
#[derive(Debug, Default)]
pub struct PropagationEngine;

impl PropagationEngine {
    /// Creates a new propagation engine instance.
    pub fn new() -> Self {
        Self
    }

    /// Propagates constraints across `domains` until fixpoint or conflict.
    ///
    /// # Complexity
    /// Time: O(E * D) worst-case AC-3 iteration where E is number of constraints, D is max domain size.
    /// Space: O(E) for work queue.
    pub fn propagate(
        &self,
        graph: &ConstraintGraph,
        domains: &mut TrailedDomains,
    ) -> PropagationResult {
        let mut queue: VecDeque<ConstraintId> = (0..graph.constraints().len())
            .map(|i| ConstraintId(i as u32))
            .collect();

        let mut in_queue: HashMap<ConstraintId, bool> =
            queue.iter().map(|&cid| (cid, true)).collect();

        let mut global_changed = false;

        while let Some(cid) = queue.pop_front() {
            in_queue.insert(cid, false);

            if let Some(constraint) = graph.get_constraint(cid) {
                match constraint.propagate(domains) {
                    PropagationResult::Conflict => return PropagationResult::Conflict,
                    PropagationResult::Success { changed } => {
                        if changed {
                            global_changed = true;
                            // Enqueue all constraints sharing variables with the scope of cid
                            for &var_id in constraint.scope() {
                                for &dep_cid in graph.constraints_for_variable(var_id) {
                                    if dep_cid != cid && !*in_queue.get(&dep_cid).unwrap_or(&false)
                                    {
                                        queue.push_back(dep_cid);
                                        in_queue.insert(dep_cid, true);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        PropagationResult::Success {
            changed: global_changed,
        }
    }
}
