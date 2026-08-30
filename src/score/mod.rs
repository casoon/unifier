//! Hard/Soft scoring model and incremental evaluation engine.
//!
//! Hard constraints must be satisfied (`hard == 0` for feasibility).
//! Soft constraints are prioritized objectives to maximize or minimize.
//!
//! References:
//! - De Causmaecker, P., et al. (2002). *The state of the art for nurse rostering problems*.
//!   Annals of Operations Research, 113, 11-38.

use crate::model::variable::VariableId;
use crate::propagation::graph::ConstraintGraph;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

/// Hard and Soft score evaluation for CSP/COP solutions.
///
/// Solutions are ordered primarily by `hard` score (0 = feasible, < 0 = violated hard constraints),
/// and secondarily by `soft` score (larger is better).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardSoftScore {
    pub hard: i64,
    pub soft: i64,
}

impl HardSoftScore {
    /// Creates a score with hard and soft components.
    ///
    /// Time & Space: O(1).
    pub fn new(hard: i64, soft: i64) -> Self {
        Self { hard, soft }
    }

    /// Returns a feasible score with hard = 0 and given soft score.
    pub fn feasible(soft: i64) -> Self {
        Self { hard: 0, soft }
    }

    /// Returns an infeasible score with given hard violation penalty.
    pub fn infeasible(hard: i64) -> Self {
        Self { hard, soft: 0 }
    }

    /// Returns `true` if all hard constraints are satisfied (`hard >= 0`).
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn is_feasible(&self) -> bool {
        self.hard >= 0
    }
}

impl Ord for HardSoftScore {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.hard.cmp(&other.hard) {
            Ordering::Equal => self.soft.cmp(&other.soft),
            ord => ord,
        }
    }
}

impl PartialOrd for HardSoftScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for HardSoftScore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_feasible() {
            write!(f, "Feasible({})", self.soft)
        } else {
            write!(f, "Infeasible(hard={}, soft={})", self.hard, self.soft)
        }
    }
}

/// Evaluator computing global and incremental scores over a constraint graph.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreCalculator;

impl ScoreCalculator {
    /// Computes the complete score for a given assignment across all constraints in `graph`.
    ///
    /// # Complexity
    /// Time: O(C) where C is number of constraints in graph.
    pub fn calculate_score(
        &self,
        graph: &ConstraintGraph,
        assignment: &HashMap<VariableId, i64>,
    ) -> HardSoftScore {
        let mut hard_violations: i64 = 0;
        let soft_score: i64 = 0;

        for constraint in graph.constraints() {
            if !constraint.is_satisfied(assignment) {
                hard_violations -= 1;
            }
        }

        HardSoftScore::new(hard_violations, soft_score)
    }

    /// Incrementally updates a score when `changed_var` is modified, re-evaluating only affected constraints.
    ///
    /// # Complexity
    /// Time: O(K) where K is number of constraints attached to `changed_var`.
    pub fn update_incremental_score(
        &self,
        graph: &ConstraintGraph,
        old_assignment: &HashMap<VariableId, i64>,
        new_assignment: &HashMap<VariableId, i64>,
        changed_var: VariableId,
        current_score: HardSoftScore,
    ) -> HardSoftScore {
        let mut hard_delta: i64 = 0;

        for &cid in graph.constraints_for_variable(changed_var) {
            if let Some(constraint) = graph.get_constraint(cid) {
                let was_satisfied = constraint.is_satisfied(old_assignment);
                let is_satisfied = constraint.is_satisfied(new_assignment);

                match (was_satisfied, is_satisfied) {
                    (false, true) => hard_delta += 1,  // violation resolved
                    (true, false) => hard_delta -= 1,  // new violation
                    _ => {}
                }
            }
        }

        HardSoftScore::new(current_score.hard + hard_delta, current_score.soft)
    }
}
