//! Local Search solver with Tabu Search memory and incremental scoring.
//!
//! Iteratively explores neighboring assignments by variable value changes and variable swaps
//! to maximize the [`crate::score::HardSoftScore`].
//!
//! References:
//! - Glover, F., & Laguna, M. (1997). *Tabu Search*. Kluwer Academic Publishers.
//! - Aarts, E., & Lenstra, J. K. (1997). *Local Search in Combinatorial Optimization*. Princeton University Press.

use crate::model::variable::VariableId;
use crate::propagation::graph::ConstraintGraph;
use crate::score::ScoreCalculator;
use crate::solver::{is_timed_out, SolveResult, SolverOptions};
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
    /// # Complexity
    /// Time: O(N * D * K) per search step where N is variables count, D is max domain size, K is affected constraints count.
    /// Space: O(N + T) where T is tabu tenure.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        let mut current_assignment = HashMap::new();

        // Generate initial assignment (min element of each variable domain)
        for (&var_id, domain) in graph.domains() {
            if let Some(min_val) = domain.min() {
                current_assignment.insert(var_id, min_val);
            } else {
                return SolveResult::Infeasible;
            }
        }

        let mut current_score = self.score_calculator.calculate_score(graph, &current_assignment);
        let mut best_assignment = current_assignment.clone();
        let mut best_score = current_score;

        let mut tabu_list: VecDeque<(VariableId, i64)> = VecDeque::with_capacity(self.tabu_tenure);
        let start_time = Instant::now();
        let mut step_count = 0u64;

        while !is_timed_out(options, start_time, step_count) {
            step_count += 1;

            if best_score.is_feasible() && best_score.hard == 0 && best_score.soft == 0 {
                // Optimal zero-violation score reached
                break;
            }

            let mut best_neighbor_move = None;
            let mut best_neighbor_score = None;
            let mut best_neighbor_assignment = None;

            // Generate single variable value change moves
            for (&var_id, domain) in graph.domains() {
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
                }
            } else {
                // Local optimum deadlock / no valid moves found
                break;
            }
        }

        if best_score.is_feasible() {
            SolveResult::Feasible {
                assignment: best_assignment,
                score: best_score,
            }
        } else if is_timed_out(options, start_time, step_count) {
            SolveResult::Timeout
        } else {
            SolveResult::Infeasible
        }
    }
}
