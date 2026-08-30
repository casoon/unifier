//! Temporal precedence constraint between scheduling intervals.
//!
//! Enforces `end(A) + min_delay <= start(B)`.
//!
//! Reference:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::interval::Interval;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Precedence constraint enforcing `end(A) + min_delay <= start(B)`.
#[derive(Debug, Clone)]
pub struct Precedence {
    end_a: VariableId,
    start_b: VariableId,
    min_delay: i64,
    scope: [VariableId; 2],
}

impl Precedence {
    /// Creates a precedence constraint between `interval_a` and `interval_b`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(interval_a: &Interval, interval_b: &Interval, min_delay: i64) -> Self {
        let end_a = interval_a.end();
        let start_b = interval_b.start();
        Self {
            end_a,
            start_b,
            min_delay,
            scope: [end_a, start_b],
        }
    }
}

impl Constraint for Precedence {
    fn name(&self) -> &str {
        "Precedence"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match (assignment.get(&self.end_a), assignment.get(&self.start_b)) {
            (Some(&ea), Some(&sb)) => ea + self.min_delay <= sb,
            _ => true,
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        let max_sb = match domains.get(&self.start_b) {
            Some(d) => match d.max() {
                Some(max) => max,
                None => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // end_a <= max_sb - min_delay
        if let Some(da) = domains.get_mut(&self.end_a) {
            if da.remove_above(max_sb - self.min_delay) {
                changed = true;
            }
            if da.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        let min_ea = match domains.get(&self.end_a) {
            Some(d) => match d.min() {
                Some(min) => min,
                None => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // start_b >= min_ea + min_delay
        if let Some(db) = domains.get_mut(&self.start_b) {
            if db.remove_below(min_ea + self.min_delay) {
                changed = true;
            }
            if db.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        PropagationResult::Success { changed }
    }
}
