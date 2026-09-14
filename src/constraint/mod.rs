//! Constraint definitions and propagation traits for CSP/COP modeling.
//!
//! References:
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.
//! - van Hoeve, W. J., & Katriel, I. (2006). *Global Constraints*. Handbook of Constraint Programming, Chapter 6.

pub mod all_different;
pub mod cardinality;
pub mod cumulative;
pub mod domain_filter;
pub mod equal;
pub mod less_than;
pub mod no_overlap;
pub mod not_equal;
pub mod optional;
pub mod periodic_values;
pub mod precedence;

pub use all_different::AllDifferent;
pub use cardinality::{AtLeast, AtMost, ExactlyOne};
pub use cumulative::{Cumulative, TaskDemand};
pub use domain_filter::{AllowedValues, ForbiddenValues};
pub use equal::Equal;
pub use less_than::LessThanOrEqual;
pub use no_overlap::NoOverlap;
pub use not_equal::NotEqual;
pub use optional::Optional;
pub use periodic_values::PeriodicValues;
pub use precedence::Precedence;

use crate::model::domain::{Domain, TrailedDomains};
use crate::model::variable::VariableId;
use std::collections::HashMap;
use std::fmt::Debug;

/// Concrete values assigned to decision variables.
pub type Assignment = HashMap<VariableId, i64>;

/// Structured explanation of one violated constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// Stable constraint type name.
    pub constraint_name: &'static str,
    /// Variables that concretely participate in the violation.
    pub involved: Vec<VariableId>,
    /// Human-readable English explanation intended for logs or direct display.
    pub message: String,
}

/// Result of a domain propagation step executed by a constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropagationResult {
    /// Propagation succeeded without encountering an empty domain.
    /// `changed` is true if any variable domain was pruned.
    Success { changed: bool },

    /// Domain reduction produced an empty domain (conflict/inconsistency detected).
    Conflict,
}

/// Core interface for constraints in `unifier`.
///
/// Each constraint defines its variable scope, satisfaction checking logic,
/// and filtering/propagation rule.
pub trait Constraint: Debug + Send + Sync {
    /// Returns a human-readable name of the constraint type.
    fn name(&self) -> &str;

    /// Returns the slice of variable IDs involved in this constraint.
    ///
    /// Time complexity: O(1).
    fn scope(&self) -> &[VariableId];

    /// Evaluates if the constraint is satisfied under a complete or partial variable assignment.
    ///
    /// Returns `true` if all variables in scope are assigned and satisfy the condition,
    /// or if unassigned variables do not violate the constraint yet.
    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool;

    /// Explains a concrete violation, or returns `None` when the assignment does not violate
    /// this constraint or the implementation has no specialized explanation.
    ///
    /// # Complexity
    /// At most the complexity of [`Self::is_satisfied`]; implementations may inspect the
    /// constraint scope once more to identify the concrete participants.
    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        let _ = assignment;
        None
    }

    /// Enforces arc/bounds consistency by pruning inconsistent values from variable domains.
    ///
    /// `domains` is a [`TrailedDomains`], not a bare `HashMap`: mutations made through
    /// [`TrailedDomains::get_mut`] are recorded so the search solvers can undo a node in
    /// `O(changed)` instead of cloning the full domain map at every node.
    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult;

    /// Validates the constraint's own parameters independent of any assignment or domain state.
    ///
    /// Returns `Err(reason)` for structurally invalid parameters (e.g. a fixed demand exceeding a
    /// fixed capacity), as opposed to a model that merely turns out to be unsatisfiable through
    /// the interaction of several constraints. Called once by [`ConstraintGraph::validate`]
    /// before solving; the default implementation accepts any parameters.
    ///
    /// [`ConstraintGraph::validate`]: crate::propagation::graph::ConstraintGraph::validate
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }

    /// Returns `false` only if this constraint can *provably* never be satisfied by any
    /// completion consistent with the current `domains`, regardless of how the as-yet-unassigned
    /// variables in its scope are eventually assigned.
    ///
    /// Used by [`crate::solver::BranchAndBoundSolver`] (via
    /// [`crate::score::ScoreCalculator::optimistic_score`]) as an admissible hard-score bound for
    /// pruning: returning `false` here marks a search subtree as unable to ever become feasible,
    /// so it must never be `false` merely because the constraint isn't satisfied *yet*.
    ///
    /// The default implementation delegates to [`Self::is_satisfied`] on `assignment`, which is
    /// correct for constraints where a violation detected from a partial assignment can never be
    /// resolved by completing it further (true for e.g. `Equal`, `NotEqual`, `AllDifferent`,
    /// `AtMost` — once violated, permanently violated). Constraints whose satisfiability
    /// genuinely depends on still-unassigned variables (e.g. `ExactlyOne`, `AtLeast`, which can
    /// still reach their target count later) MUST override this using `domains` instead.
    fn is_satisfiable(
        &self,
        domains: &HashMap<VariableId, Domain>,
        assignment: &HashMap<VariableId, i64>,
    ) -> bool {
        let _ = domains;
        self.is_satisfied(assignment)
    }
}

/// Evaluates a binary comparator over two assigned variables.
///
/// Returns `true` if either variable is unassigned (a partial assignment does not yet
/// violate a not-yet-fully-known comparison), matching the "partial assignment is not
/// violating" convention used by binary comparison constraints.
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn compare_assigned(
    assignment: &HashMap<VariableId, i64>,
    v1: VariableId,
    v2: VariableId,
    cmp: impl FnOnce(i64, i64) -> bool,
) -> bool {
    match (assignment.get(&v1), assignment.get(&v2)) {
        (Some(&val1), Some(&val2)) => cmp(val1, val2),
        _ => true,
    }
}

/// Returns the `(min, max)` bounds of `var`'s domain for propagation, distinguishing an
/// untracked variable from an already-empty domain.
///
/// Returns `Err(Success { changed: false })` if `var` is not present in `domains` (nothing to
/// prune), or `Err(Conflict)` if its domain is already empty.
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn require_bounds(
    domains: &HashMap<VariableId, Domain>,
    var: VariableId,
) -> Result<(i64, i64), PropagationResult> {
    match domains.get(&var) {
        Some(d) => match (d.min(), d.max()) {
            (Some(min), Some(max)) => Ok((min, max)),
            _ => Err(PropagationResult::Conflict),
        },
        None => Err(PropagationResult::Success { changed: false }),
    }
}

/// Returns the `(min, max)` bounds of `var`'s domain, or `None` if `var` is untracked or its
/// domain is empty. Intended for propagation loops that skip rather than abort on a missing
/// operand (e.g. global constraints iterating over a task list).
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn domain_bounds(
    domains: &HashMap<VariableId, Domain>,
    var: VariableId,
) -> Option<(i64, i64)> {
    let d = domains.get(&var)?;
    Some((d.min()?, d.max()?))
}

/// Applies `narrow` to `var`'s domain, tracking whether it removed any values in `changed` and
/// returning `Some(Conflict)` if the domain became empty. Returns `None` if `var` is untracked
/// or narrowing did not exhaust the domain, so the caller can continue.
///
/// # Complexity
/// Time & Space: O(1) plus the cost of `narrow`.
pub(crate) fn prune(
    domains: &mut TrailedDomains,
    changed: &mut bool,
    var: VariableId,
    narrow: impl FnOnce(&mut Domain) -> bool,
) -> Option<PropagationResult> {
    if domains.mutate(var, narrow)? {
        *changed = true;
    }
    if domains.get(&var)?.is_empty() {
        return Some(PropagationResult::Conflict);
    }
    None
}

/// Converts a `u64` duration to `i64` for arithmetic with `i64`-typed domain values, saturating
/// to `i64::MAX` instead of wrapping/panicking for durations exceeding `i64::MAX` (unrealistic in
/// practice, but not excluded by the `u64` type).
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn duration_as_i64(duration: u64) -> i64 {
    i64::try_from(duration).unwrap_or(i64::MAX)
}

/// Checks whether any time window `[a, b)` containing a subset of tasks demands more total
/// `demand * duration` ("energy") than a resource of `capacity` can provide across that window —
/// a generalization of pairwise mandatory-part reasoning to sets of three or more tasks whose
/// individual pairwise overlaps don't reveal an infeasibility that their *combined* demand does
/// (see `plan/11-search-heuristics-and-global-constraints.md`, part D, for a worked example: 3
/// tasks with duration 2 each and starts free in `[0,3]` overload a unary resource only as a
/// triple, not as any pair).
///
/// `tasks` gives each task's `(est, lct, energy)`: earliest start, latest completion
/// (`domain_max + duration`), and `demand * duration` (or just `duration` for a unary resource
/// with implicit demand 1, e.g. [`crate::constraint::NoOverlap`]). For every candidate window
/// `[a, b)` — `a`/`b` drawn from the tasks' own `est`/`lct` values, since a tighter window can
/// only ever be bounded by an actual task edge — sums the energy of every task fully confined to
/// it (`est(task) >= a && lct(task) <= b`) and compares against `capacity * (b - a)`. If it's
/// larger, the window cannot accommodate the confined tasks: `Conflict` regardless of what the
/// rest of the schedule looks like.
///
/// This is the sound *detection* half of energetic reasoning / edge-finding — it never reports
/// an overload that isn't real (soundness proof: every confined task's *entire* domain-feasible
/// range lies within `[a, b)` by construction, so its whole duration's energy consumption must
/// fall inside the window regardless of how it's actually scheduled; total energy exceeding
/// `capacity * window` is then a necessary condition for infeasibility). It intentionally skips
/// the harder *update* half (tightening `est` bounds for tasks that must run after an
/// overloaded-adjacent set) — left as future work, see `plan/11-...md` part D.
///
/// # Complexity
/// Time: O(N^3) (N candidate `a` thresholds x N candidate `b` thresholds x O(N) to sum energy
/// per window) — a straightforward, easily-verified enumeration rather than the O(N log N)
/// Theta-tree formulation the literature uses for the full algorithm. Fine at the task-list sizes
/// scheduling constraints see in this crate's benchmark corpus; a production-scale (100s of
/// tasks) implementation would want the Theta-tree instead.
/// Space: O(N).
///
/// # Reference
/// Erschler, J., & Lopez, P. (1990). *Energy-based approaches for task scheduling under time and
/// resource constraints*. Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based
/// Scheduling*. Springer (edge-finding and energetic reasoning for unary/cumulative resources).
pub(crate) fn energetic_overload(tasks: &[(i64, i64, i64)], capacity: u32) -> bool {
    let capacity = i64::from(capacity);
    for &(a, _, _) in tasks {
        for &(_, b, _) in tasks {
            if a >= b {
                continue;
            }
            let window = b - a;
            let energy_sum = tasks
                .iter()
                .filter(|&&(est, lct, _)| est >= a && lct <= b)
                .fold(0i64, |acc, &(_, _, energy)| acc.saturating_add(energy));
            if energy_sum > capacity.saturating_mul(window) {
                return true;
            }
        }
    }
    false
}
