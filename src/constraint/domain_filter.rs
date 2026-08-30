//! Explicit allowed and forbidden domain value filtering constraints.
//!
//! Reference:
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::{HashMap, HashSet};

/// Constraint enforcing `v in allowed_values`.
#[derive(Debug, Clone)]
pub struct AllowedValues {
    var: VariableId,
    allowed: HashSet<i64>,
    scope: [VariableId; 1],
}

impl AllowedValues {
    /// Creates an `AllowedValues` constraint for a variable.
    ///
    /// # Complexity
    /// Time & Space: O(K) where K is number of allowed values.
    pub fn new(var: VariableId, allowed_values: impl IntoIterator<Item = i64>) -> Self {
        let allowed: HashSet<i64> = allowed_values.into_iter().collect();
        Self {
            var,
            allowed,
            scope: [var],
        }
    }
}

impl Constraint for AllowedValues {
    fn name(&self) -> &str {
        "AllowedValues"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match assignment.get(&self.var) {
            Some(&val) => self.allowed.contains(&val),
            None => true,
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        if let Some(domain) = domains.get_mut(&self.var) {
            let current_values = domain.values();
            for val in current_values {
                if !self.allowed.contains(&val) {
                    if domain.remove(val) {
                        changed = true;
                    }
                }
            }
            if domain.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        PropagationResult::Success { changed }
    }
}

/// Constraint enforcing `v not in forbidden_values`.
#[derive(Debug, Clone)]
pub struct ForbiddenValues {
    var: VariableId,
    forbidden: HashSet<i64>,
    scope: [VariableId; 1],
}

impl ForbiddenValues {
    /// Creates a `ForbiddenValues` constraint for a variable.
    ///
    /// # Complexity
    /// Time & Space: O(K) where K is number of forbidden values.
    pub fn new(var: VariableId, forbidden_values: impl IntoIterator<Item = i64>) -> Self {
        let forbidden: HashSet<i64> = forbidden_values.into_iter().collect();
        Self {
            var,
            forbidden,
            scope: [var],
        }
    }
}

impl Constraint for ForbiddenValues {
    fn name(&self) -> &str {
        "ForbiddenValues"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match assignment.get(&self.var) {
            Some(&val) => !self.forbidden.contains(&val),
            None => true,
        }
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        if let Some(domain) = domains.get_mut(&self.var) {
            for &val in &self.forbidden {
                if domain.remove(val) {
                    changed = true;
                }
            }
            if domain.is_empty() {
                return PropagationResult::Conflict;
            }
        }

        PropagationResult::Success { changed }
    }
}
