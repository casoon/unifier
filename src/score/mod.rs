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
use std::sync::Arc;

/// Lexicographic soft-score level. Higher levels always outrank every lower level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScoreLevel {
    Strong,
    Medium,
    Weak,
}

/// Hard and Soft score evaluation for CSP/COP solutions.
///
/// Solutions are ordered primarily by `hard` score (0 = feasible, < 0 = violated hard constraints),
/// and secondarily by `soft` score (larger is better).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardSoftScore {
    pub hard: i64,
    pub strong: i64,
    pub medium: i64,
    pub weak: i64,
    /// Sum of all soft levels, retained for compatibility and reporting.
    pub soft: i64,
}

impl HardSoftScore {
    /// Creates a score with hard and soft components.
    ///
    /// Time & Space: O(1).
    pub fn new(hard: i64, soft: i64) -> Self {
        Self {
            hard,
            strong: 0,
            medium: 0,
            weak: soft,
            soft,
        }
    }

    /// Creates a fully tiered score.
    pub fn tiered(hard: i64, strong: i64, medium: i64, weak: i64) -> Self {
        Self {
            hard,
            strong,
            medium,
            weak,
            soft: strong.saturating_add(medium).saturating_add(weak),
        }
    }

    /// Returns a feasible score with hard = 0 and given soft score.
    pub fn feasible(soft: i64) -> Self {
        Self::new(0, soft)
    }

    /// Returns an infeasible score with given hard violation penalty.
    pub fn infeasible(hard: i64) -> Self {
        Self::new(hard, 0)
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
        self.hard
            .cmp(&other.hard)
            .then_with(|| self.strong.cmp(&other.strong))
            .then_with(|| self.medium.cmp(&other.medium))
            .then_with(|| self.weak.cmp(&other.weak))
            .then_with(|| self.soft.cmp(&other.soft))
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
            write!(
                f,
                "Feasible(strong={}, medium={}, weak={})",
                self.strong, self.medium, self.weak
            )
        } else {
            write!(
                f,
                "Infeasible(hard={}, strong={}, medium={}, weak={})",
                self.hard, self.strong, self.medium, self.weak
            )
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

    /// Stable category used for score drill-down.
    fn category(&self) -> &str {
        self.name()
    }

    /// Lexicographic level used to order this soft contribution.
    fn level(&self) -> ScoreLevel {
        ScoreLevel::Weak
    }

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

/// Adds a stable category and score level to an arbitrary objective.
#[derive(Debug, Clone)]
pub struct CategorizedObjective {
    category: String,
    level: ScoreLevel,
    inner: Arc<dyn Objective>,
}

impl CategorizedObjective {
    pub fn new(category: impl Into<String>, level: ScoreLevel, inner: Arc<dyn Objective>) -> Self {
        Self {
            category: category.into(),
            level,
            inner,
        }
    }
}

impl Objective for CategorizedObjective {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn level(&self) -> ScoreLevel {
        self.level
    }

    fn scope(&self) -> &[VariableId] {
        self.inner.scope()
    }

    fn evaluate(&self, assignment: &HashMap<VariableId, i64>) -> i64 {
        self.inner.evaluate(assignment)
    }

    fn optimistic_bound(&self, domains: &HashMap<VariableId, Domain>) -> i64 {
        self.inner.optimistic_bound(domains)
    }
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
        let sum = self
            .vars
            .iter()
            .filter_map(|v| assignment.get(v))
            .fold(0i64, |acc, &v| acc.saturating_add(v));
        self.weight.saturating_mul(sum)
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
            .fold(0i64, |acc, v| {
                acc.saturating_add(self.weight.saturating_mul(v))
            })
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

        let (strong, medium, weak) =
            objective_totals(graph, |objective| objective.evaluate(assignment));

        HardSoftScore::tiered(hard_violations, strong, medium, weak)
    }

    /// Computes an optimistic (upper-bound) score reachable from a partial `assignment` given the
    /// current `domains`, for Branch & Bound pruning.
    ///
    /// The `hard` component sums each constraint's `is_satisfiable` (see
    /// [`crate::constraint::Constraint::is_satisfiable`]) over `domains`:
    /// `0` while a constraint might still be satisfiable, `-1` only once it is *provably* not.
    /// This is deliberately not just `calculate_score`'s partial-assignment hard score — some
    /// constraints (`ExactlyOne`, `AtLeast`) are `false` on a partial assignment that has not
    /// *yet* reached their target but still could, and treating that as an already-realized
    /// violation would make this bound unsound (it could prune a still-winnable subtree).
    ///
    /// # Complexity
    /// Time: O(C + O) where C is number of constraints, O is number of objective terms in graph.
    pub fn optimistic_score(
        &self,
        graph: &ConstraintGraph,
        domains: &HashMap<VariableId, Domain>,
        assignment: &HashMap<VariableId, i64>,
    ) -> HardSoftScore {
        let hard: i64 = graph
            .constraints()
            .iter()
            .map(|c| {
                if c.is_satisfiable(domains, assignment) {
                    0
                } else {
                    -1
                }
            })
            .sum();
        let (strong, medium, weak) =
            objective_totals(graph, |objective| objective.optimistic_bound(domains));
        HardSoftScore::tiered(hard, strong, medium, weak)
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

        let mut strong_delta: i64 = 0;
        let mut medium_delta: i64 = 0;
        let mut weak_delta: i64 = 0;
        for objective in graph.objectives() {
            if objective.scope().contains(&changed_var) {
                let delta = objective
                    .evaluate(new_assignment)
                    .saturating_sub(objective.evaluate(old_assignment));
                match objective.level() {
                    ScoreLevel::Strong => strong_delta = strong_delta.saturating_add(delta),
                    ScoreLevel::Medium => medium_delta = medium_delta.saturating_add(delta),
                    ScoreLevel::Weak => weak_delta = weak_delta.saturating_add(delta),
                }
            }
        }

        HardSoftScore::tiered(
            current_score.hard.saturating_add(hard_delta),
            current_score.strong.saturating_add(strong_delta),
            current_score.medium.saturating_add(medium_delta),
            current_score.weak.saturating_add(weak_delta),
        )
    }
}

fn objective_totals(
    graph: &ConstraintGraph,
    value: impl Fn(&Arc<dyn Objective>) -> i64,
) -> (i64, i64, i64) {
    let mut strong = 0i64;
    let mut medium = 0i64;
    let mut weak = 0i64;
    for objective in graph.objectives() {
        let contribution = value(objective);
        match objective.level() {
            ScoreLevel::Strong => strong = strong.saturating_add(contribution),
            ScoreLevel::Medium => medium = medium.saturating_add(contribution),
            ScoreLevel::Weak => weak = weak.saturating_add(contribution),
        }
    }
    (strong, medium, weak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexicographic_levels_do_not_trade_strong_for_lower_scores() {
        let strong = HardSoftScore::tiered(0, 0, -1_000, -1_000);
        let lower = HardSoftScore::tiered(0, -1, 1_000_000, 1_000_000);
        assert!(strong > lower);
    }
}
