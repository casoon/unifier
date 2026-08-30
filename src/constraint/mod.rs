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
}
