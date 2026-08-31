//! Cardinality global constraints (`ExactlyOne`, `AtMost`, `AtLeast`).
//!
//! Restricts the number of variables in a set that can take a specific target value.
//!
//! References:
//! - van Hoeve, W. J., & Katriel, I. (2006). *Global Constraints*. Handbook of Constraint Programming, Chapter 6.
//! - Régin, J. C. (1996). *Generalized arc consistency for global cardinality constraint*. AAAI-96, 209-215.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Counts how many variables in `scope` are assigned to `target_value`.
///
/// # Complexity
/// Time: O(N) where N is `scope.len()`. Space: O(1).
fn count_at_target(
    scope: &[VariableId],
    assignment: &HashMap<VariableId, i64>,
    target_value: i64,
) -> usize {
    scope
        .iter()
        .filter(|v| assignment.get(v) == Some(&target_value))
        .count()
}

/// Counts, among `scope`, how many variables' domains still contain `target_value`
/// (`possible`) and how many are already fixed (singleton domain) to it (`fixed`).
///
/// # Complexity
/// Time: O(N) where N is `scope.len()`. Space: O(1).
fn target_reachability(
    scope: &[VariableId],
    domains: &HashMap<VariableId, Domain>,
    target_value: i64,
) -> (usize, usize) {
    let mut possible = 0;
    let mut fixed = 0;
    for &v in scope {
        if let Some(d) = domains.get(&v)
            && d.contains(target_value)
        {
            possible += 1;
            if d.len() == 1 {
                fixed += 1;
            }
        }
    }
    (possible, fixed)
}

/// Global constraint enforcing that exactly one variable in `scope` takes `target_value`.
#[derive(Debug, Clone)]
pub struct ExactlyOne {
    scope: Vec<VariableId>,
    target_value: i64,
}

impl ExactlyOne {
    /// Creates an `ExactlyOne` constraint over `variables` for `target_value`.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is number of variables.
    pub fn new(variables: impl IntoIterator<Item = VariableId>, target_value: i64) -> Self {
        Self {
            scope: variables.into_iter().collect(),
            target_value,
        }
    }
}

impl Constraint for ExactlyOne {
    fn name(&self) -> &str {
        "ExactlyOne"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        count_at_target(&self.scope, assignment, self.target_value) == 1
    }

    fn is_satisfiable(
        &self,
        domains: &HashMap<VariableId, Domain>,
        _assignment: &HashMap<VariableId, i64>,
    ) -> bool {
        // Still reachable unless more than one variable is already fixed to target_value (can
        // never come back down to exactly one), or no variable can possibly still reach it.
        let (possible, fixed) = target_reachability(&self.scope, domains, self.target_value);
        fixed <= 1 && possible >= 1
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;
        let mut fixed_target_var = None;
        let mut possible_count = 0;
        let mut last_possible_var = None;

        for &var_id in &self.scope {
            if let Some(domain) = domains.get(&var_id)
                && domain.contains(self.target_value)
            {
                possible_count += 1;
                last_possible_var = Some(var_id);
                if domain.len() == 1 {
                    if fixed_target_var.is_some() {
                        // Two variables fixed to target_value -> Conflict
                        return PropagationResult::Conflict;
                    }
                    fixed_target_var = Some(var_id);
                }
            }
        }

        if possible_count == 0 {
            return PropagationResult::Conflict;
        }

        // If one variable is fixed to target_value, prune target_value from all other variables
        if let Some(fixed_var) = fixed_target_var {
            for &var_id in &self.scope {
                if var_id != fixed_var
                    && let Some(d) = domains.get_mut(&var_id)
                {
                    if d.remove(self.target_value) {
                        changed = true;
                    }
                    if d.is_empty() {
                        return PropagationResult::Conflict;
                    }
                }
            }
        } else if possible_count == 1 {
            // Only one variable CAN take target_value -> force it to take target_value
            if let Some(only_var) = last_possible_var
                && let Some(d) = domains.get_mut(&only_var)
            {
                if d.assign(self.target_value) {
                    changed = true;
                }
                if d.is_empty() {
                    return PropagationResult::Conflict;
                }
            }
        }

        PropagationResult::Success { changed }
    }
}

/// Global constraint enforcing that at most `k` variables in `scope` take `target_value`.
#[derive(Debug, Clone)]
pub struct AtMost {
    scope: Vec<VariableId>,
    target_value: i64,
    k: usize,
}

impl AtMost {
    /// Creates an `AtMost` constraint enforcing at most `k` variables take `target_value`.
    pub fn new(
        k: usize,
        variables: impl IntoIterator<Item = VariableId>,
        target_value: i64,
    ) -> Self {
        Self {
            scope: variables.into_iter().collect(),
            target_value,
            k,
        }
    }
}

impl Constraint for AtMost {
    fn name(&self) -> &str {
        "AtMost"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        count_at_target(&self.scope, assignment, self.target_value) <= self.k
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;
        let mut fixed_count = 0;

        for &var_id in &self.scope {
            if let Some(domain) = domains.get(&var_id)
                && domain.len() == 1
                && domain.contains(self.target_value)
            {
                fixed_count += 1;
            }
        }

        if fixed_count > self.k {
            return PropagationResult::Conflict;
        }

        if fixed_count == self.k {
            // Prune target_value from all unassigned variables
            for &var_id in &self.scope {
                if let Some(d) = domains.get_mut(&var_id)
                    && d.len() > 1
                    && d.contains(self.target_value)
                {
                    if d.remove(self.target_value) {
                        changed = true;
                    }
                    if d.is_empty() {
                        return PropagationResult::Conflict;
                    }
                }
            }
        }

        PropagationResult::Success { changed }
    }
}

/// Global constraint enforcing that at least `k` variables in `scope` take `target_value`.
#[derive(Debug, Clone)]
pub struct AtLeast {
    scope: Vec<VariableId>,
    target_value: i64,
    k: usize,
}

impl AtLeast {
    /// Creates an `AtLeast` constraint enforcing at least `k` variables take `target_value`.
    pub fn new(
        k: usize,
        variables: impl IntoIterator<Item = VariableId>,
        target_value: i64,
    ) -> Self {
        Self {
            scope: variables.into_iter().collect(),
            target_value,
            k,
        }
    }
}

impl Constraint for AtLeast {
    fn name(&self) -> &str {
        "AtLeast"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        count_at_target(&self.scope, assignment, self.target_value) >= self.k
    }

    fn is_satisfiable(
        &self,
        domains: &HashMap<VariableId, Domain>,
        _assignment: &HashMap<VariableId, i64>,
    ) -> bool {
        // Still reachable as long as enough variables could still take target_value, even if
        // none has yet.
        let (possible, _fixed) = target_reachability(&self.scope, domains, self.target_value);
        possible >= self.k
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;
        let mut possible_vars = Vec::new();

        for &var_id in &self.scope {
            if let Some(domain) = domains.get(&var_id)
                && domain.contains(self.target_value)
            {
                possible_vars.push(var_id);
            }
        }

        if possible_vars.len() < self.k {
            return PropagationResult::Conflict;
        }

        if possible_vars.len() == self.k {
            // Force all possible variables to take target_value
            for var_id in possible_vars {
                if let Some(d) = domains.get_mut(&var_id) {
                    if d.assign(self.target_value) {
                        changed = true;
                    }
                    if d.is_empty() {
                        return PropagationResult::Conflict;
                    }
                }
            }
        }

        PropagationResult::Success { changed }
    }
}
