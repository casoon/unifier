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
use crate::propagation::graph::ConstraintGraph;
use crate::solver::backtracking::BacktrackingSolver;
use crate::solver::{SolveResult, SolverOptions, check_abort};
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
    /// # Complexity
    /// Time: O(I * d^K) where I is number of LNS iterations, K is number of destroyed variables.
    /// Space: O(N * d) graph snapshot depth.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        // Step 1: Obtain initial solution via Backtracking solver
        let initial_res = self.repair_solver.solve(graph, options);
        let (mut current_assignment, current_score) = match initial_res {
            SolveResult::Feasible {
                assignment, score, ..
            } => (assignment, score),
            other => return other,
        };

        let mut best_assignment = current_assignment.clone();
        let mut best_score = current_score;

        let start_time = Instant::now();
        let mut lns_step = 0u64;

        let vars: Vec<VariableId> = graph.variables().keys().copied().collect();
        if vars.is_empty() {
            // No variables to destroy/repair; the initial solution is already optimal.
            return SolveResult::Feasible {
                assignment: best_assignment,
                score: best_score,
                proven_optimal: false,
            };
        }
        let n_destroy = ((vars.len() as f64) * self.destroy_fraction).max(1.0) as usize;

        while check_abort(options, start_time, lns_step).is_none() {
            lns_step += 1;

            // Destroy phase: Freeze (1 - destroy_fraction) variables, unassign the remaining
            let mut sub_graph = graph.clone();
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

            // Repair phase: Solve sub-problem via Backtracking solver
            let repair_options = SolverOptions {
                time_limit: options
                    .time_limit
                    .map(|t| t.saturating_sub(start_time.elapsed())),
                max_nodes: Some(500),
                cancellation_token: options.cancellation_token.clone(),
            };

            let repair_res = self.repair_solver.solve(&sub_graph, &repair_options);
            if let SolveResult::Feasible {
                assignment, score, ..
            } = repair_res
                && score > best_score
            {
                best_score = score;
                best_assignment = assignment.clone();
                current_assignment = assignment;
            }
        }

        SolveResult::Feasible {
            assignment: best_assignment,
            score: best_score,
            proven_optimal: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propagation::graph::ConstraintGraph;

    #[test]
    fn test_solve_empty_graph_does_not_panic() {
        // Regression test: a graph with zero variables must not panic (division by zero
        // in the destroy-phase modulo) and should return the trivially feasible solution.
        let graph = ConstraintGraph::new();
        let solver = LnsSolver::default();
        let res = solver.solve(&graph, &SolverOptions::default());
        assert!(matches!(res, SolveResult::Feasible { .. }));
    }
}
