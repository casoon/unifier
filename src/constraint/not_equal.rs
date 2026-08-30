//! Inequality constraint `v1 != v2`.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann.

use crate::constraint::{Constraint, PropagationResult};
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
        match (assignment.get(&self.v1), assignment.get(&self.v2)) {
            (Some(&val1), Some(&val2)) => val1 != val2,
            _ => true,
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        // If v1 is assigned (len == 1), remove its value from v2
        if let Some(d1) = domains.get(&self.v1) {
            if d1.len() == 1 {
                if let Some(val1) = d1.min() {
                    if let Some(d2) = domains.get_mut(&self.v2) {
                        if d2.remove(val1) {
                            changed = true;
                        }
                        if d2.is_empty() {
                            return PropagationResult::Conflict;
                        }
                    }
                }
            }
        }

        // If v2 is assigned (len == 1), remove its value from v1
        if let Some(d2) = domains.get(&self.v2) {
            if d2.len() == 1 {
                if let Some(val2) = d2.min() {
                    if let Some(d1) = domains.get_mut(&self.v1) {
                        if d1.remove(val2) {
                            changed = true;
                        }
                        if d1.is_empty() {
                            return PropagationResult::Conflict;
                        }
                    }
                }
            }
        }

        PropagationResult::Success { changed }
    }
}
