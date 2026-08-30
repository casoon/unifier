//! Inequality constraint `v1 <= v2 + offset`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Constraint enforcing `v1 <= v2 + offset`.
#[derive(Debug, Clone)]
pub struct LessThanOrEqual {
    v1: VariableId,
    v2: VariableId,
    offset: i64,
    scope: [VariableId; 2],
}

impl LessThanOrEqual {
    /// Creates a constraint enforcing `v1 <= v2 + offset`.
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

impl Constraint for LessThanOrEqual {
    fn name(&self) -> &str {
        "LessThanOrEqual"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match (assignment.get(&self.v1), assignment.get(&self.v2)) {
            (Some(&val1), Some(&val2)) => val1 <= val2 + self.offset,
            _ => true,
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        let max2 = match domains.get(&self.v2) {
            Some(d) => match d.max() {
                Some(max) => max,
                None => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // v1 <= max2 + offset -> prune v1 above (max2 + offset)
        if let Some(d1) = domains.get_mut(&self.v1) {
            if d1.remove_above(max2 + self.offset) {
                changed = true;
            }
            if d1.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        let min1 = match domains.get(&self.v1) {
            Some(d) => match d.min() {
                Some(min) => min,
                None => return PropagationResult::Conflict,
            },
            None => return PropagationResult::Success { changed: false },
        };

        // v2 >= min1 - offset -> prune v2 below (min1 - offset)
        if let Some(d2) = domains.get_mut(&self.v2) {
            if d2.remove_below(min1 - self.offset) {
                changed = true;
            }
            if d2.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        PropagationResult::Success { changed }
    }
}
