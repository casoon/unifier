//! Hard/Soft scoring model and incremental evaluation engine.
//!
//! Hard constraints must be satisfied (`hard == 0` for feasibility).
//! Soft constraints are prioritized objectives to maximize or minimize.
//!
//! References:
//! - De Causmaecker, P., et al. (2002). *The state of the art for nurse rostering problems*.
//!   Annals of Operations Research, 113, 11-38.

use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use crate::propagation::graph::ConstraintGraph;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;

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

/// A weighted soft objective contributing to a solution's `soft` score.
///
/// Unlike [`crate::constraint::Constraint`], an objective is never "violated" — it contributes a
/// signed value that solvers try to maximize (a minimization goal is expressed with a negative
/// weight).
///
/// Reference:
/// - Land, A. H., & Doig, A. G. (1960). *An automatic method of solving discrete programming
///   problems*. Econometrica, 28(3), 497-520. (optimistic bounding for Branch & Bound)
pub trait Objective: Debug + Send + Sync {
    /// Returns a human-readable name of the objective.
    fn name(&self) -> &str;

    /// Returns the slice of variable IDs this objective depends on.
    ///
    /// Time complexity: O(1).
    fn scope(&self) -> &[VariableId];

    /// Evaluates this objective's contribution to the `soft` score under a complete or partial
    /// assignment. Variables absent from `assignment` contribute nothing yet, mirroring
    /// [`crate::constraint::Constraint::is_satisfied`]'s partial-assignment convention.
    fn evaluate(&self, assignment: &HashMap<VariableId, i64>) -> i64;

    /// Returns an optimistic (never-underestimating) upper bound on this objective's
    /// contribution given the current, possibly not-yet-singleton, `domains`.
    ///
    /// Used by [`crate::solver::BranchAndBoundSolver`] to prune subtrees that provably cannot
    /// improve on the best solution found so far. Must satisfy: for every completion of the
    /// current domains, `evaluate(completion) <= optimistic_bound(domains)`.
    fn optimistic_bound(&self, domains: &HashMap<VariableId, Domain>) -> i64;
}

/// Objective enforcing a weighted linear sum of variables: `weight * sum(vars)`.
///
/// A positive `weight` maximizes the sum, a negative `weight` minimizes it.
///
/// # Complexity
/// `evaluate`/`optimistic_bound`: O(N) where N is `vars.len()`.
#[derive(Debug, Clone)]
pub struct WeightedSum {
    vars: Vec<VariableId>,
    weight: i64,
}

impl WeightedSum {
    /// Creates a `WeightedSum` objective over `vars` with the given `weight`.
    pub fn new(vars: impl IntoIterator<Item = VariableId>, weight: i64) -> Self {
        Self {
            vars: vars.into_iter().collect(),
            weight,
        }
    }
}

impl Objective for WeightedSum {
    fn name(&self) -> &str {
        "WeightedSum"
    }

    fn scope(&self) -> &[VariableId] {
        &self.vars
    }

    fn evaluate(&self, assignment: &HashMap<VariableId, i64>) -> i64 {
        let sum: i64 = self.vars.iter().filter_map(|v| assignment.get(v)).sum();
        self.weight * sum
    }

    fn optimistic_bound(&self, domains: &HashMap<VariableId, Domain>) -> i64 {
        self.vars
            .iter()
            .map(|v| match domains.get(v) {
                Some(d) if !d.is_empty() => {
                    // Positive weight: largest value maximizes the product.
                    // Negative weight: smallest value maximizes the (less negative) product.
                    let extreme = if self.weight >= 0 { d.max() } else { d.min() };
                    extreme.unwrap_or(0)
                }
                _ => 0,
            })
            .map(|v| self.weight * v)
            .sum()
    }
}

/// Evaluator computing global and incremental scores over a constraint graph.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreCalculator;

impl ScoreCalculator {
    /// Computes the complete score for a given assignment across all constraints and objectives
    /// in `graph`.
    ///
    /// # Complexity
    /// Time: O(C + O) where C is number of constraints, O is number of objective terms in graph.
    pub fn calculate_score(
        &self,
        graph: &ConstraintGraph,
        assignment: &HashMap<VariableId, i64>,
    ) -> HardSoftScore {
        let mut hard_violations: i64 = 0;

        for constraint in graph.constraints() {
            if !constraint.is_satisfied(assignment) {
                hard_violations -= 1;
            }
        }

        let soft_score: i64 = graph
            .objectives()
            .iter()
            .map(|o| o.evaluate(assignment))
            .sum();

        HardSoftScore::new(hard_violations, soft_score)
    }

    /// Computes an optimistic (upper-bound) score reachable from a partial `assignment` given the
    /// current `domains`, for Branch & Bound pruning.
    ///
    /// The `hard` component reuses [`Self::calculate_score`]'s partial-assignment hard score: since
    /// every built-in constraint treats a not-yet-fully-assigned scope as not-yet-violated, hard
    /// violations can only be discovered as the assignment is completed, so the partial value is
    /// itself a valid upper bound (never smaller in magnitude than the true final `hard`).
    ///
    /// # Complexity
    /// Time: O(C + O) where C is number of constraints, O is number of objective terms in graph.
    pub fn optimistic_score(
        &self,
        graph: &ConstraintGraph,
        domains: &HashMap<VariableId, Domain>,
        assignment: &HashMap<VariableId, i64>,
    ) -> HardSoftScore {
        let partial = self.calculate_score(graph, assignment);
        let soft_bound: i64 = graph
            .objectives()
            .iter()
            .map(|o| o.optimistic_bound(domains))
            .sum();
        HardSoftScore::new(partial.hard, soft_bound)
    }

    /// Incrementally updates a score when `changed_var` is modified, re-evaluating only affected
    /// constraints and objectives.
    ///
    /// # Complexity
    /// Time: O(K + O) where K is number of constraints attached to `changed_var`, O is number of
    /// objective terms in graph.
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
                    (false, true) => hard_delta += 1, // violation resolved
                    (true, false) => hard_delta -= 1, // new violation
                    _ => {}
                }
            }
        }

        let mut soft_delta: i64 = 0;
        for objective in graph.objectives() {
            if objective.scope().contains(&changed_var) {
                soft_delta +=
                    objective.evaluate(new_assignment) - objective.evaluate(old_assignment);
            }
        }

        HardSoftScore::new(
            current_score.hard + hard_delta,
            current_score.soft + soft_delta,
        )
    }
}
