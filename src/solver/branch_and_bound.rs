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
use crate::solver::{AbortReason, SolveResult, SolverOptions, check_abort, select_mrv_variable};
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
    /// The returned [`SolveResult::Feasible::proven_optimal`] is `true` only if the search space
    /// was exhaustively explored or bound-pruned without being aborted by a time/node limit or
    /// cancellation — see `search` below.
    ///
    /// # Complexity
    /// Time: O(d^n) worst-case, reduced by bound-based pruning (see [`ScoreCalculator::optimistic_score`])
    /// and MRV variable ordering.
    /// Space: O(n * d) recursion stack depth.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        let mut current_domains = graph.domains().clone();
        let mut assignment = HashMap::new();
        let start_time = Instant::now();
        let mut nodes_count = 0u64;

        let mut best_solution = None;
        let mut best_score = None;

        if let PropagationResult::Conflict = self.propagator.propagate(graph, &mut current_domains)
        {
            return SolveResult::Infeasible;
        }

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
            },
        );

        match (best_solution, best_score) {
            (Some(assignment), Some(score)) => SolveResult::Feasible {
                assignment,
                score,
                proven_optimal: exhaustive,
            },
            _ if exhaustive => SolveResult::Infeasible,
            _ => {
                let reason =
                    check_abort(options, start_time, nodes_count).unwrap_or(AbortReason::Timeout);
                SolveResult::Aborted { reason }
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
        domains: &mut HashMap<VariableId, Domain>,
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
                }
            }
            return true;
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

        let var_id = match select_mrv_variable(graph, domains, assignment) {
            Some(v) => v,
            None => return true,
        };

        let candidate_values = match domains.get(&var_id) {
            Some(d) => d.values(),
            None => return true,
        };

        let mut exhaustive = true;
        for val in candidate_values {
            let domain_snapshot = domains.clone();

            assignment.insert(var_id, val);
            if let Some(d) = domains.get_mut(&var_id) {
                d.assign(val);
            }

            if let PropagationResult::Success { .. } = self.propagator.propagate(graph, domains) {
                exhaustive &= self.search(graph, domains, assignment, options, start_time, state);
            }

            assignment.remove(&var_id);
            *domains = domain_snapshot;
        }

        exhaustive
    }
}
