//! Equality constraint `v1 = v2 + offset`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult, compare_assigned, prune, require_bounds};
use crate::model::domain::TrailedDomains;
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
        compare_assigned(assignment, self.v1, self.v2, |val1, val2| {
            val1 == val2.saturating_add(self.offset)
        })
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let mut changed = false;

        let (min2, max2) = match require_bounds(domains, self.v2) {
            Ok(bounds) => bounds,
            Err(result) => return result,
        };

        // Prune v1 domain bounds based on v2 + offset
        if let Some(result) = prune(domains, &mut changed, self.v1, |d| {
            let below = d.remove_below(min2.saturating_add(self.offset));
            let above = d.remove_above(max2.saturating_add(self.offset));
            below || above
        }) {
            return result;
        }

        let (min1, max1) = match require_bounds(domains, self.v1) {
            Ok(bounds) => bounds,
            Err(result) => return result,
        };

        // Prune v2 domain bounds based on v1 - offset
        if let Some(result) = prune(domains, &mut changed, self.v2, |d| {
            let below = d.remove_below(min1.saturating_sub(self.offset));
            let above = d.remove_above(max1.saturating_sub(self.offset));
            below || above
        }) {
            return result;
        }

        PropagationResult::Success { changed }
    }
}
