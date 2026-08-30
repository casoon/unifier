//! Equality constraint `v1 = v2 + offset`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Constraint enforcing `v1 = v2 + offset`.
#[derive(Debug, Clone)]
pub struct Equal {
    v1: VariableId,
    v2: VariableId,
    offset: i64,
    scope: [VariableId; 2],
}

impl Equal {
    /// Creates a constraint enforcing `v1 = v2 + offset`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(v1: VariableId, v2: VariableId, offset: i64) -> Self {
        Self {
            v1,
            v2,
            offset,
            scope: [v1, v2],
        }
    }
}

impl Constraint for Equal {
    fn name(&self) -> &str {
        "Equal"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match (assignment.get(&self.v1), assignment.get(&self.v2)) {
            (Some(&val1), Some(&val2)) => val1 == val2 + self.offset,
            _ => true, // Partial assignment is not yet violating
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        let (min2, max2) = match domains.get(&self.v2) {
            Some(d) => match (d.min(), d.max()) {
                (Some(min), Some(max)) => (min, max),
                _ => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // Prune v1 domain bounds based on v2 + offset
        if let Some(d1) = domains.get_mut(&self.v1) {
            if d1.remove_below(min2 + self.offset) {
                changed = true;
            }
            if d1.remove_above(max2 + self.offset) {
                changed = true;
            }
            if d1.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        let (min1, max1) = match domains.get(&self.v1) {
            Some(d) => match (d.min(), d.max()) {
                (Some(min), Some(max)) => (min, max),
                _ => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // Prune v2 domain bounds based on v1 - offset
        if let Some(d2) = domains.get_mut(&self.v2) {
            if d2.remove_below(min1 - self.offset) {
                changed = true;
            }
            if d2.remove_above(max1 - self.offset) {
                changed = true;
            }
            if d2.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        PropagationResult::Success { changed }
    }
}
