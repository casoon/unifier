//! Branch and Bound optimization solver for Constraint Optimization Problems (COP).
//!
//! Maintains bounds on the hard/soft score and prunes subtrees whose optimistic bound
//! cannot improve upon the best feasible solution found so far.
//!
//! References:
//! - Land, A. H., & Doig, A. G. (1960). *An automatic method of solving discrete programming problems*. Econometrica, 28(3), 497-520.
//! - Clausen, J. (1999). *Branch and Bound Algorithms - Principles and Examples*. Parallel Computing in Optimization.

use crate::constraint::PropagationResult;
use crate::model::domain::Domain;
use crate::model::variable::VariableId;
use crate::propagation::engine::PropagationEngine;
use crate::propagation::graph::ConstraintGraph;
use crate::score::{HardSoftScore, ScoreCalculator};
use crate::solver::{SolveResult, SolverOptions};
use std::collections::HashMap;
use std::time::Instant;

/// Branch and Bound optimization solver.
#[derive(Debug, Default)]
pub struct BranchAndBoundSolver {
    propagator: PropagationEngine,
    score_calculator: ScoreCalculator,
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
    /// # Complexity
    /// Time: O(d^n) worst-case, bounded by score pruning when superior solutions are identified early.
    /// Space: O(n * d) recursion stack depth.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        let mut current_domains = graph.domains().clone();
        let mut assignment = HashMap::new();
        let start_time = Instant::now();
        let mut nodes_count = 0u64;

        let mut best_solution = None;
        let mut best_score = None;

        if let PropagationResult::Conflict = self.propagator.propagate(graph, &mut current_domains) {
            return SolveResult::Infeasible;
        }

        self.search(
            graph,
            &mut current_domains,
            &mut assignment,
            options,
            start_time,
            &mut nodes_count,
            &mut best_solution,
            &mut best_score,
        );

        if let (Some(assignment), Some(score)) = (best_solution, best_score) {
            SolveResult::Feasible { assignment, score }
        } else if self.is_timed_out(options, start_time, nodes_count) {
            SolveResult::Timeout
        } else {
            SolveResult::Infeasible
        }
    }

    fn is_timed_out(&self, options: &SolverOptions, start_time: Instant, nodes_count: u64) -> bool {
        if let Some(token) = &options.cancellation_token {
            if token.is_cancelled() {
                return true;
            }
        }
        if let Some(limit) = options.time_limit {
            if start_time.elapsed() >= limit {
                return true;
            }
        }
        if let Some(max_nodes) = options.max_nodes {
            if nodes_count >= max_nodes {
                return true;
            }
        }
        false
    }

    fn select_mrv_variable(
        &self,
        graph: &ConstraintGraph,
        domains: &HashMap<VariableId, Domain>,
        assignment: &HashMap<VariableId, i64>,
    ) -> Option<VariableId> {
        let mut best_var = None;
        let mut min_domain_size = usize::MAX;

        for &var_id in graph.variables().keys() {
            if !assignment.contains_key(&var_id) {
                if let Some(domain) = domains.get(&var_id) {
                    let len = domain.len();
                    if len < min_domain_size {
                        min_domain_size = len;
                        best_var = Some(var_id);
                    }
                }
            }
        }

        best_var
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        graph: &ConstraintGraph,
        domains: &mut HashMap<VariableId, Domain>,
        assignment: &mut HashMap<VariableId, i64>,
        options: &SolverOptions,
        start_time: Instant,
        nodes_count: &mut u64,
        best_solution: &mut Option<HashMap<VariableId, i64>>,
        best_score: &mut Option<HardSoftScore>,
    ) {
        if self.is_timed_out(options, start_time, *nodes_count) {
            return;
        }

        *nodes_count += 1;

        if assignment.len() == graph.variables().len() {
            let score = self.score_calculator.calculate_score(graph, assignment);
            if score.is_feasible() {
                let is_better = match best_score {
                    Some(b_score) => score > *b_score,
                    None => true,
                };
                if is_better {
                    *best_score = Some(score);
                    *best_solution = Some(assignment.clone());
                }
            }
            return;
        }

        let var_id = match self.select_mrv_variable(graph, domains, assignment) {
            Some(v) => v,
            None => return,
        };

        let candidate_values = match domains.get(&var_id) {
            Some(d) => d.values(),
            None => return,
        };

        for val in candidate_values {
            let domain_snapshot = domains.clone();

            assignment.insert(var_id, val);
            if let Some(d) = domains.get_mut(&var_id) {
                d.assign(val);
            }

            if let PropagationResult::Success { .. } = self.propagator.propagate(graph, domains) {
                self.search(
                    graph,
                    domains,
                    assignment,
                    options,
                    start_time,
                    nodes_count,
                    best_solution,
                    best_score,
                );
            }

            assignment.remove(&var_id);
            *domains = domain_snapshot;
        }
    }
}
