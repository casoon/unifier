//! Branch and Bound optimization solver for Constraint Optimization Problems (COP).
//!
//! Maintains bounds on the hard/soft score and prunes subtrees whose optimistic bound
//! cannot improve upon the best feasible solution found so far.
//!
//! References:
//! - Land, A. H., & Doig, A. G. (1960). *An automatic method of solving discrete programming problems*. Econometrica, 28(3), 497-520.
//! - Clausen, J. (1999). *Branch and Bound Algorithms - Principles and Examples*. Parallel Computing in Optimization.

use crate::constraint::PropagationResult;
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use crate::propagation::engine::PropagationEngine;
use crate::propagation::graph::{ConstraintGraph, ConstraintId, ValidatedGraph};
use crate::score::{HardSoftScore, ScoreCalculator};
use crate::solver::{
    AbortReason, SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort,
    order_values_by_neighbor_domain_size, select_dom_wdeg_variable,
};
use std::collections::HashMap;
use std::time::Instant;

/// Branch and Bound optimization solver.
#[derive(Debug, Default)]
pub struct BranchAndBoundSolver {
    propagator: PropagationEngine,
    score_calculator: ScoreCalculator,
}

/// Mutable bookkeeping threaded through the recursive [`BranchAndBoundSolver::search`] descent,
/// bundled to keep the recursive call's argument count manageable.
struct SearchState<'a> {
    nodes_count: &'a mut u64,
    best_solution: &'a mut Option<HashMap<VariableId, i64>>,
    best_score: &'a mut Option<HardSoftScore>,
    /// Per-constraint conflict counts driving the `dom/wdeg` heuristic (see
    /// [`select_dom_wdeg_variable`]). Updated by [`PropagationEngine::propagate`].
    weights: &'a mut HashMap<ConstraintId, u32>,
}

impl BranchAndBoundSolver {
    /// Creates a new Branch and Bound solver.
    pub fn new() -> Self {
        Self {
            propagator: PropagationEngine::new(),
            score_calculator: ScoreCalculator,
        }
    }

    /// Solves the COP problem, exploring the search space for the best feasible solution.
    ///
    /// The returned status is [`crate::solver::SolveStatus::Optimal`] only if the search space
    /// was exhaustively explored or bound-pruned without being aborted by a time/node limit or
    /// cancellation — see `search` below. `outcome.bound` is the root node's optimistic bound,
    /// computed once before branching; it stays loose (not tightened during search) but is always
    /// a valid upper bound on the achievable soft score.
    ///
    /// If `options.shared_incumbent` is set (see [`crate::solver::SharedIncumbent`], used by
    /// [`crate::solver::ParallelSolver`]), this solver both bounds its own search against
    /// whatever a portfolio sibling has already found and contributes its own improvements back
    /// — a solution's origin (this call or another worker) doesn't affect the returned status:
    /// exhausting the search space while holding a portfolio-wide incumbent still proves it
    /// `Optimal`.
    ///
    /// # Complexity
    /// Time: O(d^n) worst-case, reduced by bound-based pruning (see [`ScoreCalculator::optimistic_score`]),
    /// `dom/wdeg` variable ordering and least-constraining-value ordering (see
    /// [`select_dom_wdeg_variable`], [`order_values_by_neighbor_domain_size`]).
    /// Space: O(n * d) recursion stack depth.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let mut current_domains = TrailedDomains::new(graph.domains().clone());
        let mut assignment = HashMap::new();
        let start_time = Instant::now();
        let mut nodes_count = 0u64;
        let mut weights = HashMap::new();

        let mut best_solution = None;
        let mut best_score = None;

        if let PropagationResult::Conflict =
            self.propagator
                .propagate(graph, &mut current_domains, Some(&mut weights))
        {
            return SolveOutcome::infeasible(SearchStatistics {
                nodes_expanded: 0,
                elapsed: start_time.elapsed(),
            });
        }

        let root_bound =
            self.score_calculator
                .optimistic_score(graph, &current_domains, &assignment);

        let exhaustive = self.search(
            graph,
            &mut current_domains,
            &mut assignment,
            options,
            start_time,
            &mut SearchState {
                nodes_count: &mut nodes_count,
                best_solution: &mut best_solution,
                best_score: &mut best_score,
                weights: &mut weights,
            },
        );
        let statistics = SearchStatistics {
            nodes_expanded: nodes_count,
            elapsed: start_time.elapsed(),
        };

        match (best_solution, best_score) {
            (Some(assignment), Some(score)) => {
                let solution = Solution { assignment, score };
                if exhaustive {
                    // Proven optimal: the bound and the achieved score coincide (gap = 0).
                    SolveOutcome::optimal(solution, statistics, Some(score))
                } else {
                    SolveOutcome::feasible(solution, statistics, Some(root_bound))
                }
            }
            _ if exhaustive => SolveOutcome::infeasible(statistics),
            _ => {
                let reason =
                    check_abort(options, start_time, nodes_count).unwrap_or(AbortReason::Timeout);
                SolveOutcome::aborted(reason, statistics)
            }
        }
    }

    /// Recursively explores assignments of unassigned variables, applying bound-based pruning.
    ///
    /// Returns `true` if this subtree was resolved exhaustively — either by full enumeration, by
    /// a propagation conflict, or by proving via [`ScoreCalculator::optimistic_score`] that no
    /// completion of this branch can beat `state.best_score` — and `false` if it was cut short by
    /// [`check_abort`]. A pruned branch still counts as resolved: the bound is a proof, not a guess.
    fn search(
        &self,
        graph: &ConstraintGraph,
        domains: &mut TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
        options: &SolverOptions,
        start_time: Instant,
        state: &mut SearchState,
    ) -> bool {
        if check_abort(options, start_time, *state.nodes_count).is_some() {
            return false;
        }

        *state.nodes_count += 1;

        if assignment.len() == graph.variables().len() {
            let score = self.score_calculator.calculate_score(graph, assignment);
            if score.is_feasible() {
                let is_better = match state.best_score {
                    Some(b_score) => score > *b_score,
                    None => true,
                };
                if is_better {
                    *state.best_score = Some(score);
                    *state.best_solution = Some(assignment.clone());
                    if let Some(incumbent) = &options.shared_incumbent {
                        incumbent.offer(assignment, score);
                    }
                }
            }
            return true;
        }

        // Adopt a better portfolio-wide incumbent (see `SharedIncumbent`) before bounding: some
        // other worker (e.g. Local Search, LNS) may have found a stronger solution than this
        // subtree knows about yet. `best_solution` must be updated alongside `best_score` so the
        // pair stays consistent — `solve()`'s final match on `(best_solution, best_score)` would
        // otherwise report `Infeasible` despite a solution existing, if only the score were
        // adopted.
        if let Some(incumbent) = &options.shared_incumbent
            && let Some((shared_assignment, shared_score)) = incumbent.best()
            && shared_score.is_feasible()
            && state.best_score.is_none_or(|b| shared_score > b)
        {
            *state.best_score = Some(shared_score);
            *state.best_solution = Some(shared_assignment);
        }

        // Bound-based pruning: if no completion of this branch can beat the best score found so
        // far, the branch is resolved without exploring it further.
        if let Some(best) = state.best_score {
            let bound = self
                .score_calculator
                .optimistic_score(graph, domains, assignment);
            if bound <= *best {
                return true;
            }
        }

        let var_id = match select_dom_wdeg_variable(graph, domains, assignment, state.weights) {
            Some(v) => v,
            None => return true,
        };

        let candidate_values = match domains.get(&var_id) {
            Some(d) => order_values_by_neighbor_domain_size(
                graph,
                domains,
                assignment,
                var_id,
                d.values(),
            ),
            None => return true,
        };

        let mut exhaustive = true;
        for val in candidate_values {
            let checkpoint = domains.checkpoint();

            assignment.insert(var_id, val);
            if let Some(d) = domains.get_mut(&var_id) {
                d.assign(val);
            }

            // Propagate from what this node changed: the parent's domains are already a
            // fixpoint, so only this variable's constraints can have anything left to say.
            if let PropagationResult::Success { .. } = self.propagator.propagate_from(
                graph,
                domains,
                graph.constraints_for_variable(var_id).iter().copied(),
                Some(state.weights),
            ) {
                exhaustive &= self.search(graph, domains, assignment, options, start_time, state);
            }

            assignment.remove(&var_id);
            domains.undo_to(checkpoint);
        }

        exhaustive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::ExactlyOne;
    use crate::model::domain::Domain;
    use crate::score::WeightedSum;
    use std::sync::Arc;

    #[test]
    fn test_exactly_one_partial_hard_bound_does_not_falsely_prune_optimum() {
        // Regression test for plan/09-project-reevaluation-roadmap.md, P0: `ExactlyOne` is
        // `false` on a partial assignment with zero hits so far, even though a later assignment
        // could still satisfy it. A naive optimistic hard bound derived from that partial
        // `is_satisfied` check would wrongly treat a still-winnable branch as already violated
        // and prune it, missing the true optimum.
        //
        // x in 0..=1, y in 0..=2, ExactlyOne([x, y], target=0), maximize(x).
        // True optimum: x=1, y=0, soft=1. A broken bound reproducibly returned x=0, y=1, soft=0,
        // falsely marked `proven_optimal: true`.
        let mut graph = ConstraintGraph::new();
        let x = VariableId(0);
        let y = VariableId(1);
        graph.add_variable(
            crate::model::variable::Variable::new(x, "x"),
            Domain::range(0, 1),
        );
        graph.add_variable(
            crate::model::variable::Variable::new(y, "y"),
            Domain::range(0, 2),
        );
        graph.add_constraint(Arc::new(ExactlyOne::new([x, y], 0)));
        graph.add_objective(Arc::new(WeightedSum::new([x], 1)));
        let graph = graph.finalize().unwrap();

        let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
        assert_eq!(
            outcome.status,
            crate::solver::SolveStatus::Optimal,
            "should prove optimality on such a tiny instance"
        );
        let solution = outcome.solution.expect("Optimal status implies a solution");
        assert_eq!(
            solution.score,
            HardSoftScore::new(0, 1),
            "true optimum is x=1,y=0 with soft=1"
        );
        assert_eq!(solution.assignment[&x], 1);
        assert_eq!(solution.assignment[&y], 0);
    }

    fn maximize_x_model(max: i64) -> (ValidatedGraph, VariableId) {
        let mut graph = ConstraintGraph::new();
        let x = VariableId(0);
        graph.add_variable(
            crate::model::variable::Variable::new(x, "x"),
            Domain::range(0, max),
        );
        graph.add_objective(Arc::new(WeightedSum::new([x], 1)));
        (graph.finalize().unwrap(), x)
    }

    #[test]
    fn test_shared_incumbent_receives_branch_and_bound_improvements() {
        let (graph, x) = maximize_x_model(5);
        let incumbent = crate::solver::SharedIncumbent::new();
        let options = SolverOptions {
            shared_incumbent: Some(incumbent.clone()),
            ..SolverOptions::default()
        };

        let outcome = BranchAndBoundSolver::new().solve(&graph, &options);
        assert_eq!(outcome.status, crate::solver::SolveStatus::Optimal);
        assert_eq!(
            incumbent.best_score(),
            Some(HardSoftScore::new(0, 5)),
            "Branch & Bound's proven-optimal result must have been offered to the shared incumbent"
        );
        assert_eq!(outcome.solution.unwrap().assignment[&x], 5);
    }

    #[test]
    fn test_branch_and_bound_improves_on_a_preseeded_shared_incumbent() {
        let (graph, x) = maximize_x_model(10);
        let incumbent = crate::solver::SharedIncumbent::new();
        // Seed with a valid but suboptimal solution, as if another portfolio worker (e.g. Local
        // Search) had already found it before Branch & Bound started.
        let seeded_assignment: HashMap<VariableId, i64> = [(x, 3)].into_iter().collect();
        incumbent.offer(&seeded_assignment, HardSoftScore::new(0, 3));

        let options = SolverOptions {
            shared_incumbent: Some(incumbent.clone()),
            ..SolverOptions::default()
        };

        let outcome = BranchAndBoundSolver::new().solve(&graph, &options);
        assert_eq!(
            outcome.status,
            crate::solver::SolveStatus::Optimal,
            "adopting a suboptimal seeded incumbent as a starting bound must not stop the search \
             from finding and proving the true optimum"
        );
        let solution = outcome.solution.expect("Optimal implies a solution");
        assert_eq!(solution.score, HardSoftScore::new(0, 10));
        assert_eq!(solution.assignment[&x], 10);
        assert_eq!(incumbent.best_score(), Some(HardSoftScore::new(0, 10)));
    }
}
