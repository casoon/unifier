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
pub mod precedence;

pub use all_different::AllDifferent;
pub use cardinality::{AtLeast, AtMost, ExactlyOne};
pub use cumulative::{Cumulative, TaskDemand};
pub use domain_filter::{AllowedValues, ForbiddenValues};
pub use equal::Equal;
pub use less_than::LessThanOrEqual;
pub use no_overlap::NoOverlap;
pub use not_equal::NotEqual;
pub use precedence::Precedence;

use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::HashMap;
use std::fmt::Debug;

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

    /// Enforces arc/bounds consistency by pruning inconsistent values from variable domains.
    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult;

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
    domains: &mut HashMap<VariableId, Domain>,
    changed: &mut bool,
    var: VariableId,
    narrow: impl FnOnce(&mut Domain) -> bool,
) -> Option<PropagationResult> {
    let d = domains.get_mut(&var)?;
    if narrow(d) {
        *changed = true;
    }
    if d.is_empty() {
        return Some(PropagationResult::Conflict);
    }
    None
}
