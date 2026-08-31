//! Inequality constraint `v1 <= v2 + offset`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult, compare_assigned, prune, require_bounds};
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
        compare_assigned(assignment, self.v1, self.v2, |val1, val2| {
            val1 <= val2 + self.offset
        })
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        let (_, max2) = match require_bounds(domains, self.v2) {
            Ok(bounds) => bounds,
            Err(result) => return result,
        };

        // v1 <= max2 + offset -> prune v1 above (max2 + offset)
        if let Some(result) = prune(domains, &mut changed, self.v1, |d| {
            d.remove_above(max2 + self.offset)
        }) {
            return result;
        }

        let (min1, _) = match require_bounds(domains, self.v1) {
            Ok(bounds) => bounds,
            Err(result) => return result,
        };

        // v2 >= min1 - offset -> prune v2 below (min1 - offset)
        if let Some(result) = prune(domains, &mut changed, self.v2, |d| {
            d.remove_below(min1 - self.offset)
        }) {
            return result;
        }

        PropagationResult::Success { changed }
    }
}
