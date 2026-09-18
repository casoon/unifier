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
        weights: Option<&mut HashMap<ConstraintId, u32>>,
    ) -> PropagationResult {
        self.propagate_from(
            graph,
            domains,
            (0..graph.constraints().len()).map(|i| ConstraintId(i as u32)),
            weights,
        )
    }

    /// Propagates from `seeds` only, instead of from every constraint in the graph.
    ///
    /// For a caller that knows what changed — a search node that just assigned one variable
    /// seeds that variable's constraints. The rest of the graph is already at a fixpoint and
    /// re-running it finds nothing: the parent node left the domains arc-consistent, and
    /// [`TrailedDomains::undo_to`] restores exactly that state on the way back up, so the
    /// invariant holds for every node of the descent. Only what hangs off the changed variable
    /// can break, and the event-based re-enqueueing below carries that outwards as far as it
    /// actually reaches.
    ///
    /// The difference is a large constant factor on real models: seeded with the whole graph, a
    /// search node costs one [`crate::constraint::Constraint::propagate`] call per constraint in
    /// the *model*, however small the change that node made.
    ///
    /// # Complexity
    /// Time: O(E + S + R * D) where E is the constraint count (the membership flags), S the seed
    /// count and R the constraints actually reached — against O(E * D) for [`Self::propagate`],
    /// which reaches all of them by construction.
    /// Space: O(E) for the queue's membership flags.
    pub fn propagate_from(
        &self,
        graph: &ConstraintGraph,
        domains: &mut TrailedDomains,
        seeds: impl IntoIterator<Item = ConstraintId>,
        mut weights: Option<&mut HashMap<ConstraintId, u32>>,
    ) -> PropagationResult {
        // Indexed, not hashed: `ConstraintId` is a dense index into `constraints()`, so hashing
        // one per enqueue is pure overhead on the hottest path the solvers have.
        let mut in_queue = vec![false; graph.constraints().len()];
        let mut queue: VecDeque<ConstraintId> = VecDeque::new();
        for cid in seeds {
            if let Some(queued) = in_queue.get_mut(cid.0 as usize)
                && !*queued
            {
                *queued = true;
                queue.push_back(cid);
            }
        }

        let mut global_changed = false;

        while let Some(cid) = queue.pop_front() {
            in_queue[cid.0 as usize] = false;

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
                                    if dep_cid != cid && !in_queue[dep_cid.0 as usize] {
                                        queue.push_back(dep_cid);
                                        in_queue[dep_cid.0 as usize] = true;
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
