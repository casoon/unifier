//! Inequality constraint `v1 != v2 + offset`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult, compare_assigned, prune};
use crate::model::domain::{Domain, TrailedDomains};
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Constraint enforcing `v1 != v2 + offset`.
#[derive(Debug, Clone)]
pub struct NotEqual {
    v1: VariableId,
    v2: VariableId,
    offset: i64,
    scope: [VariableId; 2],
}

impl NotEqual {
    /// Creates a constraint enforcing `v1 != v2`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(v1: VariableId, v2: VariableId) -> Self {
        Self::with_offset(v1, v2, 0)
    }

    /// Creates a constraint enforcing `v1 != v2 + offset`.
    ///
    /// Useful for e.g. N-Queens-style diagonal-distinctness constraints: `q[i] != q[j] + (j - i)`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn with_offset(v1: VariableId, v2: VariableId, offset: i64) -> Self {
        Self {
            v1,
            v2,
            offset,
            scope: [v1, v2],
        }
    }
}

impl Constraint for NotEqual {
    fn name(&self) -> &str {
        "NotEqual"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        compare_assigned(assignment, self.v1, self.v2, |val1, val2| {
            val1 != val2.saturating_add(self.offset)
        })
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let mut changed = false;

        // If v1 is assigned (len == 1), remove (val1 - offset) from v2
        if let Some(val1) = domains
            .get(&self.v1)
            .filter(|d| d.len() == 1)
            .and_then(Domain::min)
            && let Some(result) = prune(domains, &mut changed, self.v2, |d| {
                d.remove(val1.saturating_sub(self.offset))
            })
        {
            return result;
        }

        // If v2 is assigned (len == 1), remove (val2 + offset) from v1
        if let Some(val2) = domains
            .get(&self.v2)
            .filter(|d| d.len() == 1)
            .and_then(Domain::min)
            && let Some(result) = prune(domains, &mut changed, self.v1, |d| {
                d.remove(val2.saturating_add(self.offset))
            })
        {
            return result;
        }

        PropagationResult::Success { changed }
    }
}
