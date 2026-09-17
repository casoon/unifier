//! Local Search solver with Tabu Search memory and incremental scoring.
//!
//! Iteratively explores neighboring assignments by variable value changes and variable swaps
//! to maximize the [`crate::score::HardSoftScore`].
//!
//! References:
//! - Glover, F., & Laguna, M. (1997). *Tabu Search*. Kluwer Academic Publishers.
//! - Aarts, E., & Lenstra, J. K. (1997). *Local Search in Combinatorial Optimization*. Princeton University Press.

use crate::model::variable::VariableId;
use crate::propagation::graph::ValidatedGraph;
use crate::score::ScoreCalculator;
use crate::solver::{
    AbortReason, SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort,
};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Local search solver utilizing tabu search memory and move neighborhoods.
#[derive(Debug)]
pub struct LocalSearchSolver {
    score_calculator: ScoreCalculator,
    tabu_tenure: usize,
}

impl Default for LocalSearchSolver {
    fn default() -> Self {
        Self {
            score_calculator: ScoreCalculator,
            tabu_tenure: 10,
        }
    }
}

impl LocalSearchSolver {
    /// Creates a local search solver with a specified tabu memory size.
    pub fn new(tabu_tenure: usize) -> Self {
        Self {
            score_calculator: ScoreCalculator,
            tabu_tenure,
        }
    }

    /// Solves the COP problem using Local Search and Tabu memory.
    ///
    /// If `options.shared_incumbent` is set (see [`crate::solver::SharedIncumbent`], used by
    /// [`crate::solver::ParallelSolver`]), every improving solution found is also offered to it —
    /// write-only: unlike [`crate::solver::BranchAndBoundSolver`], this solver has no
    /// bound-pruning to benefit from reading it back.
    ///
    /// # Complexity
    /// Time: O(N * D * K) per search step where N is variables count, D is max domain size, K is affected constraints count.
    /// Space: O(N + T) where T is tabu tenure.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let start_time = Instant::now();
        let mut current_assignment = HashMap::new();

        // Generate initial assignment (min element of each variable domain)
        for (&var_id, domain) in graph.domains() {
            if let Some(min_val) = domain.min() {
                current_assignment.insert(var_id, min_val);
            } else {
                return SolveOutcome::infeasible(SearchStatistics {
                    nodes_expanded: 0,
                    elapsed: start_time.elapsed(),
                });
            }
        }

        let mut current_score = self
            .score_calculator
            .calculate_score(graph, &current_assignment);
        let mut best_assignment = current_assignment.clone();
        let mut best_score = current_score;

        let mut tabu_list: VecDeque<(VariableId, i64)> = VecDeque::with_capacity(self.tabu_tenure);
        let mut step_count = 0u64;
        let mut deadlocked = false;

        while check_abort(options, start_time, step_count).is_none() {
            step_count += 1;

            if best_score.is_feasible() && graph.objectives().is_empty() {
                // Pure CSP (no soft objectives registered): any feasible assignment is already
                // as good as it gets, nothing left to improve. For a genuine COP, there is no
                // cheap proof of optimality available to Local Search the way Branch & Bound has
                // a bound — letting the tabu search continue until deadlock/time-limit/node-limit
                // is correct there.
                //
                // The previous condition checked `best_score.soft == 0` instead of
                // `graph.objectives().is_empty()`: that's wrong for a real objective — soft == 0
                // merely happens to be the *initial* score for many maximize-style models (e.g.
                // every variable starting at its domain minimum), not evidence that no further
                // improvement exists. That bug caused Local Search to silently return the
                // unoptimized starting assignment whenever it happened to score exactly 0.
                break;
            }

            let mut best_neighbor_move = None;
            let mut best_neighbor_score = None;
            let mut best_neighbor_assignment = None;

            // Generate single variable value change moves
            for (&var_id, domain) in graph.domains() {
                // Indexing is safe: `current_assignment` starts as a complete assignment (every
                // `graph.domains()` key) from the initial Backtracking solve, and every move
                // below only overwrites an existing key's value, never removes one.
                let current_val = current_assignment[&var_id];
                for candidate_val in domain.values() {
                    if candidate_val == current_val {
                        continue;
                    }

                    // Check tabu status
                    let is_tabu = tabu_list.contains(&(var_id, candidate_val));

                    let mut neighbor_assignment = current_assignment.clone();
                    neighbor_assignment.insert(var_id, candidate_val);

                    let neighbor_score = self.score_calculator.update_incremental_score(
                        graph,
                        &current_assignment,
                        &neighbor_assignment,
                        var_id,
                        current_score,
                    );

                    // Aspiration criterion: allow tabu move if it improves overall best score
                    let passes_aspiration = is_tabu && neighbor_score > best_score;

                    if !is_tabu || passes_aspiration {
                        let is_better = match best_neighbor_score {
                            Some(b_score) => neighbor_score > b_score,
                            None => true,
                        };
                        if is_better {
                            best_neighbor_score = Some(neighbor_score);
                            best_neighbor_move = Some((var_id, current_val));
                            best_neighbor_assignment = Some(neighbor_assignment);
                        }
                    }
                }
            }

            // Apply best non-tabu neighbor move found
            if let (Some(next_assignment), Some(next_score), Some(applied_move)) = (
                best_neighbor_assignment,
                best_neighbor_score,
                best_neighbor_move,
            ) {
                current_assignment = next_assignment;
                current_score = next_score;

                // Update tabu list
                if tabu_list.len() >= self.tabu_tenure {
                    tabu_list.pop_front();
                }
                tabu_list.push_back(applied_move);

                // Update best global solution
                if current_score > best_score {
                    best_score = current_score;
                    best_assignment = current_assignment.clone();
                    // Only *feasible* solutions belong in the portfolio (as in `LnsSolver`):
                    // this solver's starting assignment violates hard constraints in all but
                    // the smallest models, and a portfolio that adopts one reports it as its
                    // solution.
                    if best_score.is_feasible()
                        && let Some(incumbent) = &options.shared_incumbent
                    {
                        incumbent.offer(&best_assignment, best_score);
                    }
                }
            } else {
                // Local optimum deadlock / no valid moves found
                deadlocked = true;
                break;
            }
        }

        let statistics = SearchStatistics {
            nodes_expanded: step_count,
            elapsed: start_time.elapsed(),
        };

        if best_score.is_feasible() {
            SolveOutcome::feasible(
                Solution {
                    assignment: best_assignment,
                    score: best_score,
                },
                statistics,
                None,
            )
        } else if deadlocked {
            SolveOutcome::aborted(AbortReason::LocalOptimum, statistics)
        } else {
            // The while condition became false: check_abort must have returned Some.
            let reason =
                check_abort(options, start_time, step_count).unwrap_or(AbortReason::Timeout);
            SolveOutcome::aborted(reason, statistics)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;
    use crate::propagation::graph::ConstraintGraph;
    use crate::score::WeightedSum;
    use std::sync::Arc;
    use std::time::Duration;

    /// Regression test for a bug found while adding the shared-incumbent wiring above: the old
    /// early-break condition checked `best_score.soft == 0`, intending "a fully feasible pure-CSP
    /// solution needs no further search" — but for a genuine maximize/minimize objective, `soft
    /// == 0` is just whatever the *initial* assignment happens to score (e.g. every variable at
    /// its domain minimum), not evidence of optimality. That silently returned the unoptimized
    /// starting assignment instead of searching at all.
    #[test]
    fn test_solve_does_not_stop_early_when_initial_score_happens_to_be_zero() {
        let mut graph = ConstraintGraph::new();
        let x = VariableId(0);
        graph.add_variable(
            crate::model::variable::Variable::new(x, "x"),
            Domain::range(0, 5),
        );
        graph.add_objective(Arc::new(WeightedSum::new([x], 1)));
        let graph = graph.finalize().unwrap();

        // A short time limit is enough: for this trivial model, the first search step already
        // climbs straight to x = 5 (tabu search otherwise runs until timeout here, since it keeps
        // taking non-improving moves rather than deadlocking — a pre-existing property of this
        // solver, not something these tests need to wait out).
        let options = SolverOptions {
            time_limit: Some(Duration::from_millis(200)),
            ..SolverOptions::default()
        };
        let outcome = LocalSearchSolver::default().solve(&graph, &options);
        let solution = outcome
            .solution
            .expect("feasible: single unconstrained variable");
        assert_eq!(
            solution.assignment[&x], 5,
            "x starts at domain min 0 (soft score 0); the old buggy check stopped right there \
             instead of climbing to the true best, x = 5"
        );
    }

    #[test]
    fn test_solve_offers_final_solution_to_shared_incumbent() {
        // Whatever Local Search's tabu/neighborhood search actually converges on (not asserted
        // here — that's an algorithm-quality question, not a wiring one), the returned
        // solution's score must match what was offered to the shared incumbent: every
        // improvement found along the way is mirrored to it.
        let mut graph = ConstraintGraph::new();
        let x = VariableId(0);
        graph.add_variable(
            crate::model::variable::Variable::new(x, "x"),
            Domain::range(0, 5),
        );
        graph.add_objective(Arc::new(WeightedSum::new([x], 1)));
        let graph = graph.finalize().unwrap();

        let incumbent = crate::solver::SharedIncumbent::new();
        let options = SolverOptions {
            time_limit: Some(Duration::from_millis(200)),
            shared_incumbent: Some(incumbent.clone()),
            ..SolverOptions::default()
        };
        let outcome = LocalSearchSolver::default().solve(&graph, &options);
        let solution = outcome
            .solution
            .expect("feasible: single unconstrained variable");
        assert_eq!(incumbent.best_score(), Some(solution.score));
    }
}
