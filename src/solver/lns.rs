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
use crate::solver::branch_and_bound::BranchAndBoundSolver;
use crate::solver::local_search::LocalSearchSolver;
use crate::solver::{
    AbortReason, SearchStatistics, Solution, SolveOutcome, SolverOptions, check_abort,
};
use crate::{Assignment, ScoreCalculator};
use std::thread;
use std::time::{Duration, Instant};

/// How long a portfolio worker waits for a sibling to produce something to repair before it
/// builds a center itself. Local Search reaches its first complete assignment in milliseconds
/// even on large models, so this is generous; the bound exists so that a model where nobody
/// produces anything cannot leave this solver idling instead of working.
const CENTER_WAIT: Duration = Duration::from_millis(250);

/// The largest share of a run that may go into waiting for one.
const CENTER_WAIT_DIVISOR: u32 = 10;

/// Variables in a related neighbourhood.
const RELATED_SIZE: usize = 12;

/// Nodes the exact search may spend on one related neighbourhood.
const RELATED_NODES: u64 = 2_000;

/// How often that wait looks.
const CENTER_POLL: Duration = Duration::from_millis(2);

/// Large Neighborhood Search solver.
#[derive(Debug)]
pub struct LnsSolver {
    repair_solver: BacktrackingSolver,
    /// The sub-search used while the center still breaks hard constraints — see the repair phase
    /// in [`LnsSolver::improve_from`] for why an exact one cannot serve there.
    conflict_repair: LocalSearchSolver,
    /// The exact sub-search over a related neighbourhood once the center is feasible.
    optimising_repair: BranchAndBoundSolver,
    destroy_fraction: f64,
}

impl Default for LnsSolver {
    fn default() -> Self {
        Self {
            repair_solver: BacktrackingSolver::new(),
            conflict_repair: LocalSearchSolver::default(),
            optimising_repair: BranchAndBoundSolver::new(),
            destroy_fraction: 0.3,
        }
    }
}

impl LnsSolver {
    /// Creates a new LNS solver with specified destroy fraction `0.0..1.0`.
    pub fn new(destroy_fraction: f64) -> Self {
        Self {
            repair_solver: BacktrackingSolver::new(),
            conflict_repair: LocalSearchSolver::default(),
            optimising_repair: BranchAndBoundSolver::new(),
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
        let start_time = Instant::now();

        // In a portfolio this solver repairs; it does not construct. A sibling worker is already
        // running the very tree search a construction here would run, and on a model too large
        // for that search to reach a first complete assignment, duplicating it wastes the one
        // worker that could have started from a near miss instead.
        if let Some(incumbent) = &options.shared_incumbent {
            // Never more than a small share of the run, and never long in absolute terms. A
            // handle without siblings behind it — a caller passing one in, a short budget — must
            // not turn into a solver that waits instead of working.
            let wait = options.time_limit.map_or(CENTER_WAIT, |limit| {
                CENTER_WAIT.min(limit / CENTER_WAIT_DIVISOR)
            });
            while start_time.elapsed() < wait && check_abort(options, start_time, 0).is_none() {
                if let Some((assignment, score)) = incumbent.center()
                    && Self::covers_every_variable(graph, &assignment)
                {
                    return self.improve_from(
                        graph,
                        Solution { assignment, score },
                        &Self::remaining(options, start_time),
                    );
                }
                thread::sleep(CENTER_POLL);
            }
            // Nobody produced anything to repair. Build a center after all rather than idle.
        }

        let initial_outcome = self
            .repair_solver
            .solve(graph, &Self::remaining(options, start_time));
        match initial_outcome.solution {
            Some(initial) => {
                self.improve_from(graph, initial, &Self::remaining(options, start_time))
            }
            None => initial_outcome,
        }
    }

    /// The options with the time limit reduced by what has already been spent, so that a phase
    /// handed the "rest" of a run cannot start a full budget over again.
    /// A seed variable and the variables that share constraints with it, breadth first and
    /// tightest constraint first, up to [`RELATED_SIZE`]. The seed walks through the variables
    /// with the step, so every one of them is a seed in turn.
    fn related_neighbourhood(
        graph: &ValidatedGraph,
        sorted_vars: &[VariableId],
        step: u64,
    ) -> Vec<VariableId> {
        // A prime stride visits every variable before repeating, without walking neighbours.
        let seed = sorted_vars[(step as usize).wrapping_mul(7_919) % sorted_vars.len()];
        let mut related = vec![seed];
        let mut next = 0;
        while next < related.len() && related.len() < RELATED_SIZE {
            let var = related[next];
            next += 1;
            let mut constraints: Vec<&[VariableId]> = graph
                .constraints_for_variable(var)
                .iter()
                .filter_map(|id| graph.get_constraint(*id))
                .map(|constraint| constraint.scope())
                .collect();
            constraints.sort_by_key(|scope| scope.len());
            for scope in constraints {
                for other in scope {
                    if related.len() == RELATED_SIZE {
                        break;
                    }
                    if !related.contains(other) {
                        related.push(*other);
                    }
                }
            }
        }
        related
    }

    fn remaining(options: &SolverOptions, start_time: Instant) -> SolverOptions {
        let mut remaining = options.clone();
        remaining.time_limit = options
            .time_limit
            .map(|limit| limit.saturating_sub(start_time.elapsed()));
        remaining
    }

    /// Whether `assignment` gives every variable a value its domain still allows — what the
    /// destroy phase needs in order to freeze the part it keeps.
    fn covers_every_variable(graph: &ValidatedGraph, assignment: &Assignment) -> bool {
        graph.variables().keys().all(|variable| {
            assignment
                .get(variable)
                .is_some_and(|value| graph.domains()[variable].contains(*value))
        })
    }

    /// Repairs from a caller-provided baseline assignment. The baseline is used as the LNS
    /// neighborhood center even when changed constraints make it infeasible.
    pub fn solve_from(
        &self,
        graph: &ValidatedGraph,
        baseline: &Assignment,
        options: &SolverOptions,
    ) -> SolveOutcome {
        if !Self::covers_every_variable(graph, baseline) {
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

        // The center this run starts from, feasible or not: whoever handed it over already knows
        // it, but a caller-supplied baseline (`solve_from`) may be news to the portfolio.
        if let Some(incumbent) = &options.shared_incumbent {
            incumbent.offer(&current_assignment, current_score);
        }

        let start_time = Instant::now();
        let mut lns_step = 0u64;

        let vars: Vec<VariableId> = graph.variables().keys().copied().collect();
        let mut sorted_vars = vars.clone();
        sorted_vars.sort_unstable();
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

            let remaining = options
                .time_limit
                .map(|t| t.saturating_sub(start_time.elapsed()));

            // Which sub-search runs depends on what the center is.
            //
            // While it breaks hard constraints, the search has to *lower* the number of
            // violations rather than demand none — an exact one demands none and reports nothing
            // (measured: nothing in thirty seconds from 18 violations). And it gets the whole
            // model, not a neighbourhood. Repairing a displaced activity takes a chain: it moves
            // onto a slot whose occupant moves on in turn. A destroy step freezes most of those
            // occupants, and frozen on a value a new restriction forbids, a variable even empties
            // its own domain at the first propagation. Measured on a blocked period in a full
            // timetable (`schedulr/tests/repair_after_block.rs`, timbra plan 53): with 30 % of the
            // variables free per step, 8 displaced lessons stayed 6–7 conflicts for the whole
            // budget; with the whole model free, all 8 moved and nothing else. The node cap goes
            // too — a chain is a run of consecutive steps, and cutting it at 500 to start over
            // from the old center throws it away.
            //
            // Once the center is feasible the question changes: every step has to *stay*
            // feasible while improving the objective, and that is what the exact search over a
            // destroyed neighbourhood is for.
            let repair_outcome = if current_score.is_feasible() && lns_step % 2 == 0 {
                // Every other step: a small neighbourhood of related variables, re-optimised
                // exactly. A slice of the variables in id order rarely holds two that have to
                // change together — two activities trading their rooms — and a first-solution
                // search over it ignores the objective, so it only ever stumbles on an
                // improvement.
                let related = Self::related_neighbourhood(graph, &sorted_vars, lns_step);
                let mut sub_graph = graph.graph().clone();
                let mut sub_domains = sub_graph.domains().clone();
                for v in &sorted_vars {
                    if !related.contains(v)
                        && let Some(&assigned_val) = current_assignment.get(v)
                        && let Some(d) = sub_domains.get_mut(v)
                    {
                        d.assign(assigned_val);
                    }
                }
                *sub_graph.domains_mut() = sub_domains;
                let sub_graph = ValidatedGraph::assume_valid(sub_graph);
                self.optimising_repair.solve(
                    &sub_graph,
                    &SolverOptions {
                        time_limit: remaining,
                        max_nodes: Some(RELATED_NODES),
                        cancellation_token: options.cancellation_token.clone(),
                        seed: options.seed,
                        shared_incumbent: None,
                    },
                )
            } else if current_score.is_feasible() {
                // Destroy phase: freeze (1 - destroy_fraction) of the variables. A
                // search-internal derivative of an already-validated graph, so it is re-wrapped
                // via `assume_valid` rather than re-running `validate`.
                let mut sub_graph = graph.graph().clone();
                let mut sub_domains = sub_graph.domains().clone();
                let destroy_offset = (lns_step as usize) % vars.len();
                for i in n_destroy..vars.len() {
                    let v = vars[(i + destroy_offset) % vars.len()];
                    if let Some(&assigned_val) = current_assignment.get(&v)
                        && let Some(d) = sub_domains.get_mut(&v)
                    {
                        d.assign(assigned_val);
                    }
                }
                *sub_graph.domains_mut() = sub_domains;
                let sub_graph = ValidatedGraph::assume_valid(sub_graph);
                self.repair_solver.solve(
                    &sub_graph,
                    &SolverOptions {
                        time_limit: remaining,
                        max_nodes: Some(500),
                        cancellation_token: options.cancellation_token.clone(),
                        seed: options.seed,
                        shared_incumbent: options.shared_incumbent.clone(),
                    },
                )
            } else {
                self.conflict_repair.repair_from(
                    graph,
                    &current_assignment,
                    &SolverOptions {
                        time_limit: remaining,
                        max_nodes: None,
                        cancellation_token: options.cancellation_token.clone(),
                        seed: options.seed,
                        shared_incumbent: options.shared_incumbent.clone(),
                    },
                )
            };
            // `reached()`, not `solution`: a repair that merely got closer is the whole point,
            // and it arrives as the fallback rather than as a result (plan 51, C2).
            let Some(candidate) = repair_outcome.reached() else {
                continue;
            };
            if candidate.score > current_score {
                current_score = candidate.score;
                current_assignment = candidate.assignment.clone();
                // Shared whether or not it is feasible: a repaired near miss is a better place
                // for the next worker to start than the one it has, and `SharedIncumbent` keeps
                // a starting point apart from an answer.
                if let Some(incumbent) = &options.shared_incumbent {
                    incumbent.offer(&current_assignment, current_score);
                }
            }
            if candidate.score.is_feasible()
                && best
                    .as_ref()
                    .is_none_or(|best| candidate.score > best.score)
            {
                best = Some(candidate.clone());
            }
        }

        let statistics = SearchStatistics {
            nodes_expanded: lns_step,
            elapsed: start_time.elapsed(),
        };
        match best {
            Some(solution) => SolveOutcome::feasible(solution, statistics, None),
            // The loop above only ends when the budget does, so there is nothing left to pay for
            // another search. Starting one here would spend a second full time limit on top of
            // the one already used, and say no more than this does — except for how far the
            // repairing got, which rides along as the center this run ended on.
            None => SolveOutcome::aborted(
                check_abort(options, start_time, lns_step).unwrap_or(AbortReason::Timeout),
                statistics,
            )
            .with_best_effort(Some(Solution {
                assignment: current_assignment,
                score: current_score,
            })),
        }
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

    #[test]
    fn a_baseline_broken_by_a_new_restriction_is_repaired() {
        // Every value of the baseline is now forbidden by a unary hard constraint (a new
        // calendar block inside a long horizon — beyond 4096 values propagation only trims the
        // bounds, so the value stays in the domain). Freezing such a variable makes its
        // neighbourhood unsolvable at once, so variables of violated constraints must always be
        // released.
        let mut graph = ConstraintGraph::new();
        let mut baseline = Assignment::new();
        for index in 0..20 {
            let variable = VariableId(index);
            graph.add_variable(
                crate::model::variable::Variable::new(variable, "v"),
                Domain::range(0, 10_000),
            );
            graph.add_constraint(Arc::new(
                crate::constraint::periodic_values::PeriodicValues::new(
                    variable,
                    6,
                    0..6,
                    [(3_000, 3_000)],
                ),
            ));
            baseline.insert(variable, 3_000);
        }
        let graph = graph.finalize().unwrap();
        let options = SolverOptions {
            time_limit: Some(std::time::Duration::from_millis(3000)),
            ..SolverOptions::default()
        };

        let outcome = LnsSolver::default().solve_from(&graph, &baseline, &options);
        let solution = outcome
            .solution
            .expect("the repair leaves the blocked value behind");
        assert!(solution.assignment.values().all(|value| *value != 3_000));
    }
}
