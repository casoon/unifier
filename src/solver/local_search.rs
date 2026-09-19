//! Local Search solver combining conflict-directed (min-conflicts) repair with Tabu Search
//! memory and incremental scoring.
//!
//! While hard constraints are violated, the search is *directed*: it draws one of the violated
//! constraints, moves a variable in its scope, and leaves the rest of the model untouched — the
//! cost of a step depends on that variable's domain and degree, not on the size of the model.
//! Once nothing is violated any more, and only then, it scans the full neighbourhood for soft
//! improvements, which is what an objective needs and a conflict cannot point at.
//!
//! References:
//! - Minton, S., Johnston, M. D., Philips, A. B., & Laird, P. (1992). *Minimizing conflicts: a
//!   heuristic repair method for constraint satisfaction and scheduling problems*. Artificial
//!   Intelligence, 58(1-3), 161-205.
//! - Glover, F., & Laguna, M. (1997). *Tabu Search*. Kluwer Academic Publishers.
//! - Aarts, E., & Lenstra, J. K. (1997). *Local Search in Combinatorial Optimization*. Princeton University Press.

use crate::constraint::PropagationResult;
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use crate::propagation::engine::PropagationEngine;
use crate::propagation::graph::{ConstraintId, ValidatedGraph};
use crate::score::{HardSoftScore, ScoreCalculator};
use crate::solver::{
    AbortReason, SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort,
};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// How many violated constraints one repair step draws from before committing to a move.
///
/// One draw is textbook min-conflicts, and it is cheap but blind: the variable it lands on may
/// have no good move while another conflicted variable does, so the search wanders. Sampling a
/// few and keeping the best move buys most of a full scan's judgement at a cost that still does
/// not depend on how big the model is.
const CONFLICT_SAMPLES: usize = 4;

/// Deterministic linear congruential generator for tie-breaking, seeded from
/// [`SolverOptions::seed`] — the same generator `pathwise` uses for its annealing choices.
///
/// Min-conflicts needs to break ties at random (always taking the first of several equally good
/// values walks into the same dead end every time), but a search nobody can replay is a search
/// nobody can debug or benchmark, hence the explicit seed.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// An index below `len`, which must be non-zero.
    fn index(&mut self, len: usize) -> usize {
        (self.next() >> 11) as usize % len
    }

    /// Reservoir sampling over a run of equally-scoring candidates: called with `seen` counting
    /// the candidate currently being offered (1 for the first), it says whether that candidate
    /// replaces the one held, giving every tied candidate the same chance.
    fn replaces_tied(&mut self, seen: usize) -> bool {
        self.index(seen) == 0
    }
}

/// The constraints violated by the current assignment.
///
/// A vector so one can be drawn at random in O(1), plus the position of each entry so that
/// inserting and removing while the search moves stays O(1) as well — the set changes after
/// every single step, so scanning it each time would put the model's size back into the cost of
/// a step.
#[derive(Debug, Default)]
struct ViolationSet {
    entries: Vec<ConstraintId>,
    positions: HashMap<ConstraintId, usize>,
}

impl ViolationSet {
    fn insert(&mut self, constraint: ConstraintId) {
        if self.positions.contains_key(&constraint) {
            return;
        }
        self.positions.insert(constraint, self.entries.len());
        self.entries.push(constraint);
    }

    fn remove(&mut self, constraint: ConstraintId) {
        let Some(index) = self.positions.remove(&constraint) else {
            return;
        };
        self.entries.swap_remove(index);
        if let Some(&moved) = self.entries.get(index) {
            self.positions.insert(moved, index);
        }
    }

    fn pick(&self, rng: &mut Lcg) -> Option<ConstraintId> {
        if self.entries.is_empty() {
            return None;
        }
        Some(self.entries[rng.index(self.entries.len())])
    }
}

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
    /// Time per step: O(D * (K + O)) while repairing a conflict (D is the chosen variable's
    /// domain size, K its constraint degree, O the objective count), O(N * D * (K + O)) for a
    /// soft-improvement step over all N variables. Nothing is allocated per candidate.
    /// Space: O(N + C + T) for the assignment, the violated-constraint set and the tabu tenure.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let start_time = Instant::now();
        let mut rng = Lcg::new(options.seed);

        let Some((domains, mut current_assignment)) = self.initial_assignment(graph, &mut rng)
        else {
            return SolveOutcome::infeasible(SearchStatistics {
                nodes_expanded: 0,
                elapsed: start_time.elapsed(),
            });
        };

        let mut current_score = self
            .score_calculator
            .calculate_score(graph, &current_assignment);
        let mut best_assignment = current_assignment.clone();
        let mut best_score = current_score;

        let mut violated = ViolationSet::default();
        for (index, constraint) in graph.constraints().iter().enumerate() {
            if !constraint.is_satisfied(&current_assignment) {
                violated.insert(ConstraintId(index as u32));
            }
        }

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

            // Repair before polish: while a hard constraint is violated, move one of the
            // variables it involves instead of searching the whole model for the prettiest step.
            // A conflicted variable with no move left (every candidate tabu, or nothing but its
            // current value in the domain) is not a dead end for the search, only for that draw,
            // so the full scan still gets its turn before the search gives up.
            let mut step: Option<(VariableId, i64, HardSoftScore)> = None;
            for _ in 0..CONFLICT_SAMPLES {
                let Some(var_id) = violated
                    .pick(&mut rng)
                    .and_then(|constraint| self.conflicted_variable(graph, constraint, &mut rng))
                else {
                    break;
                };
                let candidate = self.best_value_for(
                    graph,
                    &domains,
                    &mut current_assignment,
                    var_id,
                    current_score,
                    &tabu_list,
                    best_score,
                    &mut rng,
                );
                if let Some(candidate) = candidate
                    && step.is_none_or(|(_, _, held)| candidate.2 > held)
                {
                    step = Some(candidate);
                }
            }
            if step.is_none() {
                step = self.best_improving_move(
                    graph,
                    &domains,
                    &mut current_assignment,
                    current_score,
                    &tabu_list,
                    best_score,
                    &mut rng,
                );
            }

            let Some((var_id, next_value, next_score)) = step else {
                // Local optimum deadlock / no valid moves found
                deadlocked = true;
                break;
            };

            let previous_value = current_assignment.insert(var_id, next_value);
            current_score = next_score;

            // Only constraints attached to the variable that moved can have changed status.
            for &cid in graph.constraints_for_variable(var_id) {
                let satisfied = graph
                    .get_constraint(cid)
                    .is_some_and(|constraint| constraint.is_satisfied(&current_assignment));
                if satisfied {
                    violated.remove(cid);
                } else {
                    violated.insert(cid);
                }
            }

            // Tabu memory holds the value just left, so the search cannot immediately undo itself.
            if let Some(previous_value) = previous_value {
                if tabu_list.len() >= self.tabu_tenure {
                    tabu_list.pop_front();
                }
                tabu_list.push_back((var_id, previous_value));
            }

            // Update best global solution
            if current_score > best_score {
                best_score = current_score;
                best_assignment.clone_from(&current_assignment);
                // Offered whether or not it is feasible. Past the smallest models this solver
                // spends most of a run infeasible, and those near misses are exactly what a
                // repair search wants to start from. `SharedIncumbent` keeps a starting point
                // apart from an answer, so offering one can never become the portfolio's result.
                if let Some(incumbent) = &options.shared_incumbent {
                    incumbent.offer(&best_assignment, best_score);
                }
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

    /// Builds the starting assignment: root-level propagation first, then one greedy pass giving
    /// each variable the value that conflicts least with what is already placed.
    ///
    /// Returns the propagated domains alongside it, since every later candidate is drawn from
    /// them: they are never larger than the graph's own and a root-level fixpoint cannot remove
    /// a value that any solution uses.
    ///
    /// The previous starting point was each variable's domain minimum. Where values are time
    /// slots that puts every activity on the earliest slot — the most conflicted assignment the
    /// model has, and one the repair loop then has to dig its way out of. `None` means a variable
    /// has no values left at all, so no assignment exists.
    ///
    /// Constraints are judged on the partial assignment built so far, so a constraint that only
    /// becomes satisfiable once its whole scope is placed (`ExactlyOne` and friends) counts as
    /// violated throughout the pass. That biases the greedy choice but cannot mislead the search:
    /// this is a starting point, and the repair loop re-judges everything on the complete
    /// assignment.
    fn initial_assignment(
        &self,
        graph: &ValidatedGraph,
        rng: &mut Lcg,
    ) -> Option<(TrailedDomains, HashMap<VariableId, i64>)> {
        let mut domains = TrailedDomains::new(graph.domains().clone());
        if let PropagationResult::Conflict =
            PropagationEngine::new().propagate(graph, &mut domains, None)
        {
            return None;
        }

        let mut var_ids: Vec<VariableId> = graph.variables().keys().copied().collect();
        var_ids.sort_unstable();

        let mut assignment = HashMap::with_capacity(var_ids.len());
        for var_id in var_ids {
            let values = domains.get(&var_id)?.values();
            let (&first, rest) = values.split_first()?;

            assignment.insert(var_id, first);
            let mut best_value = first;
            let mut fewest = self.conflicts_at(graph, &assignment, var_id);
            let mut tied = 1usize;
            for &value in rest {
                assignment.insert(var_id, value);
                let conflicts = self.conflicts_at(graph, &assignment, var_id);
                if conflicts < fewest {
                    fewest = conflicts;
                    best_value = value;
                    tied = 1;
                } else if conflicts == fewest {
                    tied += 1;
                    if rng.replaces_tied(tied) {
                        best_value = value;
                    }
                }
            }
            assignment.insert(var_id, best_value);
        }

        Some((domains, assignment))
    }

    /// How many of `var_id`'s constraints `assignment` violates.
    fn conflicts_at(
        &self,
        graph: &ValidatedGraph,
        assignment: &HashMap<VariableId, i64>,
        var_id: VariableId,
    ) -> usize {
        graph
            .constraints_for_variable(var_id)
            .iter()
            .filter_map(|&cid| graph.get_constraint(cid))
            .filter(|constraint| !constraint.is_satisfied(assignment))
            .count()
    }

    /// Draws one of the variables a violated constraint involves — the variable min-conflicts
    /// will try to move. Constraints carry their scope, so this needs no search.
    fn conflicted_variable(
        &self,
        graph: &ValidatedGraph,
        constraint: ConstraintId,
        rng: &mut Lcg,
    ) -> Option<VariableId> {
        let scope = graph.get_constraint(constraint)?.scope();
        (!scope.is_empty()).then(|| scope[rng.index(scope.len())])
    }

    /// The best value to move `var_id` to: highest resulting score, ties broken at random, tabu
    /// values skipped unless they beat the best score found so far (aspiration).
    ///
    /// `assignment` is left exactly as it was found — candidates are scored in place.
    #[allow(clippy::too_many_arguments)]
    fn best_value_for(
        &self,
        graph: &ValidatedGraph,
        domains: &TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
        var_id: VariableId,
        current_score: HardSoftScore,
        tabu_list: &VecDeque<(VariableId, i64)>,
        best_score: HardSoftScore,
        rng: &mut Lcg,
    ) -> Option<(VariableId, i64, HardSoftScore)> {
        let current_value = *assignment.get(&var_id)?;
        let mut best: Option<(i64, HardSoftScore)> = None;
        let mut tied = 0usize;

        for candidate in domains.get(&var_id)?.values() {
            if candidate == current_value {
                continue;
            }
            let score = self.score_calculator.score_after_change(
                graph,
                assignment,
                var_id,
                candidate,
                current_score,
            );
            // Aspiration criterion: a tabu move is allowed when it beats the overall best.
            if tabu_list.contains(&(var_id, candidate)) && score <= best_score {
                continue;
            }
            match best {
                Some((_, held)) if score < held => {}
                Some((_, held)) if score == held => {
                    tied += 1;
                    if rng.replaces_tied(tied) {
                        best = Some((candidate, score));
                    }
                }
                _ => {
                    best = Some((candidate, score));
                    tied = 1;
                }
            }
        }

        best.map(|(value, score)| (var_id, value, score))
    }

    /// The best single-variable move across the whole model, for when nothing is violated and
    /// only the objectives can still be improved — there is no conflict left to aim at.
    #[allow(clippy::too_many_arguments)]
    fn best_improving_move(
        &self,
        graph: &ValidatedGraph,
        domains: &TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
        current_score: HardSoftScore,
        tabu_list: &VecDeque<(VariableId, i64)>,
        best_score: HardSoftScore,
        rng: &mut Lcg,
    ) -> Option<(VariableId, i64, HardSoftScore)> {
        let mut var_ids: Vec<VariableId> = graph.variables().keys().copied().collect();
        var_ids.sort_unstable();

        let mut best: Option<(VariableId, i64, HardSoftScore)> = None;
        for var_id in var_ids {
            let Some(candidate) = self.best_value_for(
                graph,
                domains,
                assignment,
                var_id,
                current_score,
                tabu_list,
                best_score,
                rng,
            ) else {
                continue;
            };
            if best.is_none_or(|(_, _, held)| candidate.2 > held) {
                best = Some(candidate);
            }
        }
        best
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

    /// The repair loop draws conflicts and breaks ties at random, so the same seed has to produce
    /// the same run — otherwise a benchmark measures noise and a failing run cannot be replayed.
    #[test]
    fn test_same_seed_repeats_the_same_search() {
        // A 3-colouring of a 9-node ring: enough conflicts that the repair loop actually runs and
        // has choices to make, small enough to solve instantly.
        let mut graph = ConstraintGraph::new();
        let nodes: Vec<VariableId> = (0..9).map(VariableId).collect();
        for &node in &nodes {
            graph.add_variable(
                crate::model::variable::Variable::new(node, format!("n{}", node.0)),
                Domain::range(1, 3),
            );
        }
        for i in 0..nodes.len() {
            graph.add_constraint(Arc::new(crate::constraint::NotEqual::with_offset(
                nodes[i],
                nodes[(i + 1) % nodes.len()],
                0,
            )));
        }
        let graph = graph.finalize().unwrap();

        let solve = |seed: u64| {
            LocalSearchSolver::default()
                .solve(
                    &graph,
                    &SolverOptions {
                        time_limit: Some(Duration::from_millis(200)),
                        seed,
                        ..SolverOptions::default()
                    },
                )
                .solution
                .expect("a 9-node ring is 3-colourable")
                .assignment
        };

        assert_eq!(
            solve(7),
            solve(7),
            "the same seed must replay the same search"
        );
    }
}
