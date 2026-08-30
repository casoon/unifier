//! Global `AllDifferent` constraint enforcing pairwise distinction across a set of variables.
//!
//! References:
//! - Régin, J. C. (1994). *A filtering algorithm for constraints of difference in CSPs*. AAAI-94, 362-367.
//! - van Hoeve, W. J. (2001). *The AllDifferent constraint: A survey*. arXiv:cs/0105015.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::{HashMap, HashSet};

/// Global constraint enforcing that all variables in its scope take pairwise distinct values.
#[derive(Debug, Clone)]
pub struct AllDifferent {
    scope: Vec<VariableId>,
}

impl AllDifferent {
    /// Creates a new `AllDifferent` constraint over the given variables.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is number of variables.
    pub fn new(variables: impl IntoIterator<Item = VariableId>) -> Self {
        Self {
            scope: variables.into_iter().collect(),
        }
    }
}

impl Constraint for AllDifferent {
    fn name(&self) -> &str {
        "AllDifferent"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        let mut seen = HashSet::new();
        for var in &self.scope {
            if let Some(&val) = assignment.get(var) {
                if !seen.insert(val) {
                    return false; // Duplicate value found
                }
            }
        }
        true
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        // Collect all fixed values (singleton domains)
        let mut fixed_values = HashSet::new();
        for var in &self.scope {
            if let Some(domain) = domains.get(var) {
                if domain.len() == 1 {
                    if let Some(val) = domain.min() {
                        if !fixed_values.insert(val) {
                            // Two fixed variables have the same value -> Conflict
                            return PropagationResult::Conflict;
                        }
                    }
                }
            }
        }

        if fixed_values.is_empty() {
            return PropagationResult::Success { changed: false };
        }

        // Prune fixed values from all non-fixed variables in scope
        for var in &self.scope {
            if let Some(domain) = domains.get_mut(var) {
                if domain.len() > 1 {
                    for &val in &fixed_values {
                        if domain.remove(val) {
                            changed = true;
                        }
                    }
                    if domain.is_empty() {
                        return PropagationResult::Conflict;
                    }
                }
            }
        }

        PropagationResult::Success { changed }
    }
}
