//! Large Neighborhood Search (LNS) solver combining heuristic destroy and exact repair phases.
//!
//! Repeatedly relaxes a percentage of assigned variables (Destroy) and reconstructs an optimal
//! assignment for the relaxed neighborhood using exact sub-search (Repair).
//!
//! References:
//! - Shaw, P. (1998). *Using constraint programming and local search methods to solve vehicle routing problems*.
//!   CP 1998, LNCS 1504, 417-431.
//! - Pisinger, D., & Ropke, S. (2010). *Large Neighborhood Search*. Handbook of Metaheuristics, Springer, 399-419.

use crate::model::variable::VariableId;
use crate::propagation::graph::ValidatedGraph;
use crate::solver::backtracking::BacktrackingSolver;
use crate::solver::{SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort};
use crate::{Assignment, ScoreCalculator};
use std::time::Instant;

/// Large Neighborhood Search solver.
#[derive(Debug)]
pub struct LnsSolver {
    repair_solver: BacktrackingSolver,
    destroy_fraction: f64,
}

impl Default for LnsSolver {
    fn default() -> Self {
        Self {
            repair_solver: BacktrackingSolver::new(),
            destroy_fraction: 0.3,
        }
    }
}

impl LnsSolver {
    /// Creates a new LNS solver with specified destroy fraction `0.0..1.0`.
    pub fn new(destroy_fraction: f64) -> Self {
        Self {
            repair_solver: BacktrackingSolver::new(),
            destroy_fraction: destroy_fraction.clamp(0.1, 0.9),
        }
    }

    /// Solves the COP problem using Large Neighborhood Search.
    ///
    /// If `options.shared_incumbent` is set (see [`crate::solver::SharedIncumbent`], used by
    /// [`crate::solver::ParallelSolver`]), every improving solution found (including the initial
    /// one) is also offered to it — write-only, like [`crate::solver::LocalSearchSolver`].
    ///
    /// # Complexity
    /// Time: O(I * d^K) where I is number of LNS iterations, K is number of destroyed variables.
    /// Space: O(N * d) graph snapshot depth.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let initial_outcome = self.repair_solver.solve(graph, options);
        let initial = match initial_outcome.solution {
            Some(solution) => solution,
            None => return initial_outcome,
        };
        self.improve_from(graph, initial, options)
    }

    /// Repairs from a caller-provided baseline assignment. The baseline is used as the LNS
    /// neighborhood center even when changed constraints make it infeasible.
    pub fn solve_from(
        &self,
        graph: &ValidatedGraph,
        baseline: &Assignment,
        options: &SolverOptions,
    ) -> SolveOutcome {
        let baseline_is_complete = graph.variables().keys().all(|variable| {
            baseline
                .get(variable)
                .is_some_and(|value| graph.domains()[variable].contains(*value))
        });
        if !baseline_is_complete {
            return self.solve(graph, options);
        }
        let score = ScoreCalculator.calculate_score(graph, baseline);
        self.improve_from(
            graph,
            Solution {
                assignment: baseline.clone(),
                score,
            },
            options,
        )
    }

    fn improve_from(
        &self,
        graph: &ValidatedGraph,
        initial: Solution,
        options: &SolverOptions,
    ) -> SolveOutcome {
        let mut current_assignment = initial.assignment;
        let mut current_score = initial.score;
        let mut best = current_score.is_feasible().then(|| Solution {
            assignment: current_assignment.clone(),
            score: current_score,
        });

        if let (Some(incumbent), Some(solution)) = (&options.shared_incumbent, &best) {
            incumbent.offer(&solution.assignment, solution.score);
        }

        let start_time = Instant::now();
        let mut lns_step = 0u64;

        let vars: Vec<VariableId> = graph.variables().keys().copied().collect();
        if vars.is_empty() {
            // No variables to destroy/repair; the initial solution is already optimal.
            let statistics = SearchStatistics {
                nodes_expanded: 0,
                elapsed: start_time.elapsed(),
            };
            return best.map_or_else(
                || self.repair_solver.solve(graph, options),
                |solution| SolveOutcome::feasible(solution, statistics, None),
            );
        }
        let n_destroy = ((vars.len() as f64) * self.destroy_fraction).max(1.0) as usize;

        while check_abort(options, start_time, lns_step).is_none() {
            lns_step += 1;

            // Destroy phase: Freeze (1 - destroy_fraction) variables, unassign the remaining.
            // A search-internal derivative of an already-validated graph, so it is re-wrapped
            // via `assume_valid` below rather than re-running `validate`.
            let mut sub_graph = graph.graph().clone();
            let mut sub_domains = sub_graph.domains().clone();

            let destroy_offset = (lns_step as usize) % vars.len();
            for i in 0..vars.len() {
                let v = vars[(i + destroy_offset) % vars.len()];
                if i >= n_destroy {
                    // Freeze variable v to its current assigned value
                    if let Some(&assigned_val) = current_assignment.get(&v)
                        && let Some(d) = sub_domains.get_mut(&v)
                    {
                        d.assign(assigned_val);
                    }
                }
            }

            *sub_graph.domains_mut() = sub_domains;
            let sub_graph = ValidatedGraph::assume_valid(sub_graph);

            // Repair phase: Solve sub-problem via Backtracking solver
            let repair_options = SolverOptions {
                time_limit: options
                    .time_limit
                    .map(|t| t.saturating_sub(start_time.elapsed())),
                max_nodes: Some(500),
                cancellation_token: options.cancellation_token.clone(),
                seed: options.seed,
                shared_incumbent: options.shared_incumbent.clone(),
            };

            let repair_outcome = self.repair_solver.solve(&sub_graph, &repair_options);
            if let Some(solution) = repair_outcome.solution {
                if solution.score > current_score {
                    current_score = solution.score;
                    current_assignment = solution.assignment.clone();
                }
                if solution.score.is_feasible()
                    && best.as_ref().is_none_or(|best| solution.score > best.score)
                {
                    if let Some(incumbent) = &options.shared_incumbent {
                        incumbent.offer(&solution.assignment, solution.score);
                    }
                    best = Some(solution);
                }
            }
        }

        let statistics = SearchStatistics {
            nodes_expanded: lns_step,
            elapsed: start_time.elapsed(),
        };
        best.map_or_else(
            || self.repair_solver.solve(graph, options),
            |solution| SolveOutcome::feasible(solution, statistics, None),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;
    use crate::propagation::graph::ConstraintGraph;
    use std::sync::Arc;

    #[test]
    fn test_solve_empty_graph_does_not_panic() {
        // Regression test: a graph with zero variables must not panic (division by zero
        // in the destroy-phase modulo) and should return the trivially feasible solution.
        let graph = ConstraintGraph::new().finalize().unwrap();
        let solver = LnsSolver::default();
        let outcome = solver.solve(&graph, &SolverOptions::default());
        assert!(outcome.solution.is_some());
    }

    #[test]
    fn test_solve_offers_final_solution_to_shared_incumbent() {
        // Whatever LNS's destroy/repair search actually converges on (not asserted here — that's
        // an algorithm-quality question, not a wiring one), the returned solution's score must
        // match what was offered to the shared incumbent: every improvement (including the
        // initial one) is mirrored to it.
        let mut graph = ConstraintGraph::new();
        let x = VariableId(0);
        graph.add_variable(
            crate::model::variable::Variable::new(x, "x"),
            Domain::range(0, 5),
        );
        graph.add_objective(Arc::new(crate::score::WeightedSum::new([x], 1)));
        let graph = graph.finalize().unwrap();

        let incumbent = crate::solver::SharedIncumbent::new();
        let options = SolverOptions {
            time_limit: Some(std::time::Duration::from_millis(200)),
            shared_incumbent: Some(incumbent.clone()),
            ..SolverOptions::default()
        };
        let outcome = LnsSolver::default().solve(&graph, &options);
        let solution = outcome
            .solution
            .expect("feasible: single unconstrained variable");
        assert_eq!(incumbent.best_score(), Some(solution.score));
    }
}
