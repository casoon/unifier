//! Inequality constraint `v1 != v2`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{compare_assigned, prune, Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Constraint enforcing `v1 != v2`.
#[derive(Debug, Clone)]
pub struct NotEqual {
    v1: VariableId,
    v2: VariableId,
    scope: [VariableId; 2],
}

impl NotEqual {
    /// Creates a constraint enforcing `v1 != v2`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(v1: VariableId, v2: VariableId) -> Self {
        Self {
            v1,
            v2,
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
        compare_assigned(assignment, self.v1, self.v2, |val1, val2| val1 != val2)
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        // If v1 is assigned (len == 1), remove its value from v2
        if let Some(val1) = domains.get(&self.v1).filter(|d| d.len() == 1).and_then(Domain::min) {
            if let Some(result) = prune(domains, &mut changed, self.v2, |d| d.remove(val1)) {
                return result;
            }
        }

        // If v2 is assigned (len == 1), remove its value from v1
        if let Some(val2) = domains.get(&self.v2).filter(|d| d.len() == 1).and_then(Domain::min) {
            if let Some(result) = prune(domains, &mut changed, self.v1, |d| d.remove(val2)) {
                return result;
            }
        }

        PropagationResult::Success { changed }
    }
}
