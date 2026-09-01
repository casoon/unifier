//! Arc-consistency AC-3 propagation engine.
//!
//! Maintains a queue of constraints to re-evaluate whenever variable domains shrink,
//! propagating domain reductions until a fixpoint or conflict is reached. Re-enqueueing is
//! event-based at the granularity of "which specific variable actually shrank" (see
//! `plan/11-search-heuristics-and-global-constraints.md`, part B) rather than "some variable in
//! the whole scope of the constraint that just ran" — a constraint with a wide scope that only
//! narrows one variable's domain doesn't force re-checking of constraints tied to its other,
//! untouched scope variables.
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
    /// If `weights` is provided, the weight of whichever constraint causes a conflict is
    /// incremented — used by the `dom/wdeg` search heuristic (see
    /// `plan/11-search-heuristics-and-global-constraints.md`, part A;
    /// `crate::solver::select_dom_wdeg_variable`). Pass `None` when the heuristic isn't in use.
    ///
    /// # Event-based re-enqueueing
    /// [`crate::constraint::Constraint::propagate`] only reports *whether* it changed something
    /// (`PropagationResult::Success { changed }`), not *which* scope variable(s). Rather than
    /// widening that trait — which would touch every one of the ~9 constraint implementations —
    /// this engine reads [`TrailedDomains::changed_since`] to learn exactly which variables were
    /// touched during the call (the trail is already recorded for undo, so this is a cheap slice
    /// read, not extra domain lookups). Only constraints sharing one of the variables actually
    /// touched are re-enqueued, instead of every constraint sharing *any* variable with the full
    /// scope. This is a coarser signal than the `Assigned`/`BoundsChanged`/`ValueRemoved` event
    /// taxonomy `plan/11-...md` part B sketches (it doesn't distinguish *how* a domain changed,
    /// only *that* it did for a given variable, and may over-report a variable whose net effect
    /// was a no-op — see `changed_since`'s doc comment), but delivers a measurable reduction in
    /// redundant re-propagation at effectively no extra cost, and is a straightforward base to
    /// refine further if profiling shows event-*kind* filtering would still help.
    ///
    /// An earlier version of this re-enqueueing derived "which variables changed" from an
    /// explicit before/after domain-length snapshot per call instead of the trail; benchmarking
    /// showed that *doubling* the domain lookups on every `propagate()` call cost more than the
    /// more-precise re-enqueueing saved, a net regression (see `plan/00-STATUS.md`). Reading the
    /// trail avoids the extra lookups entirely.
    ///
    /// # Complexity
    /// Time: O(E * D) worst-case AC-3 iteration where E is number of constraints, D is max domain
    /// size, plus O(K) per constraint invocation (K = trail entries recorded during that call) to
    /// read back which variables changed.
    /// Space: O(E) for work queue.
    pub fn propagate(
        &self,
        graph: &ConstraintGraph,
        domains: &mut TrailedDomains,
        mut weights: Option<&mut HashMap<ConstraintId, u32>>,
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
                let trail_checkpoint = domains.checkpoint();

                match constraint.propagate(domains) {
                    PropagationResult::Conflict => {
                        if let Some(w) = weights.as_deref_mut() {
                            *w.entry(cid).or_insert(1) += 1;
                        }
                        return PropagationResult::Conflict;
                    }
                    PropagationResult::Success { changed } => {
                        if changed {
                            global_changed = true;
                            // Enqueue dependents only of the specific variables this call
                            // actually touched (per the trail), not the constraint's whole scope.
                            for var_id in domains.changed_since(trail_checkpoint) {
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
