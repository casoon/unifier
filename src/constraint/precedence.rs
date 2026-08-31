//! Temporal precedence constraint between scheduling intervals.
//!
//! Enforces `end(A) + min_delay <= start(B)`.
//!
//! Structurally equivalent to `end(A) <= start(B) + (-min_delay)`, so this is implemented as a
//! thin wrapper around [`LessThanOrEqual`].
//!
//! Reference:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.

use crate::constraint::{Constraint, LessThanOrEqual, PropagationResult};
use crate::model::domain::TrailedDomains;
use crate::model::interval::Interval;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Precedence constraint enforcing `end(A) + min_delay <= start(B)`.
#[derive(Debug, Clone)]
pub struct Precedence {
    inner: LessThanOrEqual,
}

impl Precedence {
    /// Creates a precedence constraint between `interval_a` and `interval_b`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(interval_a: &Interval, interval_b: &Interval, min_delay: i64) -> Self {
        Self {
            inner: LessThanOrEqual::new(interval_a.end(), interval_b.start(), -min_delay),
        }
    }
}

impl Constraint for Precedence {
    fn name(&self) -> &str {
        "Precedence"
    }

    fn scope(&self) -> &[VariableId] {
        self.inner.scope()
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        self.inner.is_satisfied(assignment)
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        self.inner.propagate(domains)
    }
}
