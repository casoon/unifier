//! Backtracking CSP solver with Minimum Remaining Values (MRV / Fail-First) variable ordering
//! and AC-3 constraint propagation.
//!
//! References:
//! - Haralick, R. M., & Elliott, G. L. (1980). *Increasing tree search efficiency for constraint satisfaction problems*.
//!   Artificial Intelligence, 14(3), 263-313.
//! - Bitner, J. R., & Reingold, E. M. (1975). *Backtrack programming techniques*. CACM, 18(11), 651-656.

use crate::constraint::PropagationResult;
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use crate::propagation::engine::PropagationEngine;
use crate::propagation::graph::{ConstraintGraph, ValidatedGraph};
use crate::score::ScoreCalculator;
use crate::solver::{
    SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort, select_mrv_variable,
};
use std::collections::HashMap;
use std::time::Instant;

/// Backtracking solver with MRV heuristics and constraint propagation.
#[derive(Debug, Default)]
pub struct BacktrackingSolver {
    propagator: PropagationEngine,
    score_calculator: ScoreCalculator,
}

impl BacktrackingSolver {
    /// Creates a new backtracking solver instance.
    pub fn new() -> Self {
        Self {
            propagator: PropagationEngine::new(),
            score_calculator: ScoreCalculator,
        }
    }

    /// Solves the given constraint graph, returning the first feasible solution found or `Infeasible`.
    ///
    /// # Complexity
    /// Time: O(d^n) worst-case search tree size, mitigated by MRV variable ordering and AC-3 domain pruning.
    /// Space: O(n * d) recursion stack depth and domain snapshot storage.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let mut current_domains = TrailedDomains::new(graph.domains().clone());
        let mut assignment = HashMap::new();
        let start_time = Instant::now();
        let mut nodes_count = 0u64;

        // Initial AC-3 propagation over full graph
        if let PropagationResult::Conflict = self.propagator.propagate(graph, &mut current_domains)
        {
            return SolveOutcome::infeasible(SearchStatistics {
                nodes_expanded: 0,
                elapsed: start_time.elapsed(),
            });
        }

        let found = self.backtrack(
            graph,
            &mut current_domains,
            &mut assignment,
            options,
            start_time,
            &mut nodes_count,
        );
        let statistics = SearchStatistics {
            nodes_expanded: nodes_count,
            elapsed: start_time.elapsed(),
        };

        if found {
            let score = self.score_calculator.calculate_score(graph, &assignment);
            SolveOutcome::feasible(Solution { assignment, score }, statistics, None)
        } else if let Some(reason) = check_abort(options, start_time, nodes_count) {
            SolveOutcome::aborted(reason, statistics)
        } else {
            SolveOutcome::infeasible(statistics)
        }
    }

    fn backtrack(
        &self,
        graph: &ConstraintGraph,
        domains: &mut TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
        options: &SolverOptions,
        start_time: Instant,
        nodes_count: &mut u64,
    ) -> bool {
        if check_abort(options, start_time, *nodes_count).is_some() {
            return false;
        }

        *nodes_count += 1;

        // If all variables are assigned, verify satisfaction
        if assignment.len() == graph.variables().len() {
            let score = self.score_calculator.calculate_score(graph, assignment);
            return score.is_feasible();
        }

        // Select next variable via MRV heuristic
        let var_id = match select_mrv_variable(graph, domains, assignment) {
            Some(v) => v,
            None => return assignment.len() == graph.variables().len(),
        };

        let candidate_values = match domains.get(&var_id) {
            Some(d) => d.values(),
            None => return false,
        };

        for val in candidate_values {
            // Checkpoint the domain trail prior to assignment (O(1), no full clone).
            let checkpoint = domains.checkpoint();

            // Assign value
            assignment.insert(var_id, val);
            if let Some(d) = domains.get_mut(&var_id) {
                d.assign(val);
            }

            // Propagate constraints
            if let PropagationResult::Success { .. } = self.propagator.propagate(graph, domains)
                && self.backtrack(graph, domains, assignment, options, start_time, nodes_count)
            {
                return true;
            }

            // Backtrack: restore state (only the domains actually touched since checkpoint).
            assignment.remove(&var_id);
            domains.undo_to(checkpoint);
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{AllDifferent, Equal, NotEqual};
    use crate::model::domain::Domain;
    use crate::model::variable::{Variable, VariableId};
    use std::sync::Arc;

    #[test]
    fn test_solve_simple_equality() {
        let mut graph = ConstraintGraph::new();
        let v1 = VariableId(1);
        let v2 = VariableId(2);

        graph.add_variable(Variable::new(v1, "x"), Domain::range(1, 3));
        graph.add_variable(Variable::new(v2, "y"), Domain::range(1, 3));

        // x = y + 1
        graph.add_constraint(Arc::new(Equal::new(v1, v2, 1)));
        let graph = graph.finalize().unwrap();

        let solver = BacktrackingSolver::new();
        let outcome = solver.solve(&graph, &SolverOptions::default());

        match outcome.solution {
            Some(Solution { assignment, score }) => {
                assert!(score.is_feasible());
                let x_val = assignment.get(&v1).copied().unwrap();
                let y_val = assignment.get(&v2).copied().unwrap();
                assert_eq!(x_val, y_val + 1);
            }
            None => panic!(
                "Expected feasible solution, got status {:?}",
                outcome.status
            ),
        }
    }

    #[test]
    fn test_solve_nqueens_4() {
        // Board: `vars[row]` holds the column of the queen in that row.
        let mut graph = ConstraintGraph::new();
        let vars: Vec<VariableId> = (0..4).map(VariableId).collect();

        for &v in &vars {
            graph.add_variable(Variable::new(v, format!("q{}", v.0)), Domain::range(1, 4));
        }

        // Column distinctness: no two queens share a column.
        graph.add_constraint(Arc::new(AllDifferent::new(vars.clone())));

        // Diagonal distinctness: q[i] - q[j] != +-(j - i) for every row pair i < j.
        for i in 0..4 {
            for j in (i + 1)..4 {
                let diff = (j - i) as i64;
                graph.add_constraint(Arc::new(NotEqual::with_offset(vars[i], vars[j], diff)));
                graph.add_constraint(Arc::new(NotEqual::with_offset(vars[i], vars[j], -diff)));
            }
        }
        let graph = graph.finalize().unwrap();

        let solver = BacktrackingSolver::new();
        let outcome = solver.solve(&graph, &SolverOptions::default());

        match outcome.solution {
            Some(Solution { assignment, .. }) => {
                // Don't just trust the solver's own feasibility claim: independently verify the
                // returned assignment against both N-Queens rules.
                for i in 0..4 {
                    for j in (i + 1)..4 {
                        let qi = assignment[&vars[i]];
                        let qj = assignment[&vars[j]];
                        assert_ne!(qi, qj, "queens in row {i} and {j} share column {qi}");
                        assert_ne!(
                            (qi - qj).abs(),
                            (j - i) as i64,
                            "queens in row {i} and {j} share a diagonal"
                        );
                    }
                }
            }
            None => panic!(
                "Expected feasible N-Queens(4) solution, got status {:?}",
                outcome.status
            ),
        }
    }
}
