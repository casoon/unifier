//! Constraint satisfaction and optimization solvers.
//!
//! Provides Backtracking with MRV heuristics, Branch & Bound, Local Search, LNS, and Parallel Portfolio search.

pub mod backtracking;
pub mod branch_and_bound;
pub mod cancellation;
pub mod lns;
pub mod local_search;
pub mod parallel;
pub mod shared_incumbent;

pub use backtracking::BacktrackingSolver;
pub use branch_and_bound::BranchAndBoundSolver;
pub use cancellation::{CancellationToken, SearchStatistics};
pub use lns::LnsSolver;
pub use local_search::LocalSearchSolver;
pub use parallel::ParallelSolver;
pub use shared_incumbent::SharedIncumbent;

use crate::model::domain::{Domain, TrailedDomains};
use crate::model::variable::VariableId;
use crate::propagation::graph::{ConstraintGraph, ConstraintId};
use crate::score::HardSoftScore;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Configuration options for solver execution limits and cancellation.
#[derive(Debug, Clone)]
pub struct SolverOptions {
    /// Optional maximum duration limit.
    pub time_limit: Option<Duration>,
    /// Optional maximum number of search nodes to expand.
    pub max_nodes: Option<u64>,
    /// Optional thread-safe cancellation handle.
    pub cancellation_token: Option<CancellationToken>,
    /// Seed for the randomized tie-breaking in [`LocalSearchSolver`].
    ///
    /// Fixed by default, so two runs over the same model with the same options take the same
    /// path — without that, neither a benchmark nor a bug report is reproducible. Vary it to
    /// sample different paths through the same landscape.
    pub seed: u64,
    /// Optional portfolio-wide shared incumbent (see [`SharedIncumbent`]). When present,
    /// [`BranchAndBoundSolver`] additionally bounds its search against it (and contributes its
    /// own improvements back), and [`LocalSearchSolver`]/[`LnsSolver`] contribute improving
    /// solutions they find. Set by [`ParallelSolver`] for its workers; `None` for a standalone
    /// solver run.
    pub shared_incumbent: Option<SharedIncumbent>,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            time_limit: Some(Duration::from_secs(10)),
            max_nodes: None,
            cancellation_token: None,
            seed: 42,
            shared_incumbent: None,
        }
    }
}

/// Why a search run stopped without reaching a conclusive result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    /// The caller's [`CancellationToken`] was cancelled.
    Cancelled,
    /// `SolverOptions::time_limit` elapsed.
    Timeout,
    /// `SolverOptions::max_nodes` was reached.
    NodeLimit,
    /// A local-search-style solver reached a local optimum with no improving, non-tabu move
    /// available. This does not prove infeasibility or optimality.
    LocalOptimum,
}

/// What a solver run established about the problem.
///
/// Decoupled from whether an incumbent solution was found: `Aborted` can still carry no
/// solution, while `Feasible`/`Optimal` always do (see [`SolveOutcome::solution`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// A feasible solution was found and proven optimal: no better score is reachable.
    /// Currently only [`BranchAndBoundSolver`] can establish this.
    Optimal,
    /// A feasible solution was found, but the search ended before optimality could be proven —
    /// or the solver used (Backtracking, Local Search, LNS) cannot prove it at all.
    Feasible,
    /// The constraint network is provably unsatisfiable: the full search space was exhausted
    /// without finding any feasible assignment.
    Infeasible,
    /// The search stopped (see the contained [`AbortReason`]) before finding a feasible
    /// assignment and before the search space was exhausted, so neither feasibility nor
    /// infeasibility could be established.
    Aborted(AbortReason),
}

/// A candidate solution: a complete variable assignment and its score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solution {
    pub assignment: HashMap<VariableId, i64>,
    pub score: HardSoftScore,
}

/// Outcome of a solver run: what was established, the best solution found (if any), search
/// effort spent, and — where the solver tracks one — an optimistic bound on the achievable score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveOutcome {
    /// What the search established. See [`SolveStatus`].
    pub status: SolveStatus,
    /// The best solution found. Present for [`SolveStatus::Optimal`] and
    /// [`SolveStatus::Feasible`]; absent for [`SolveStatus::Infeasible`] and
    /// [`SolveStatus::Aborted`].
    pub solution: Option<Solution>,
    /// Search effort spent producing this outcome.
    pub statistics: SearchStatistics,
    /// An upper bound on the achievable `soft` score, when the solver computes one. Currently
    /// only [`BranchAndBoundSolver`] populates this (from the root node's optimistic bound,
    /// which may be loose if the search was aborted before narrowing it further). `None` for
    /// solvers that do not track a bound (Backtracking, Local Search, LNS, Parallel).
    pub bound: Option<HardSoftScore>,
    /// The best **complete** assignment the search reached when it could not return one as a
    /// solution. Vouching for an assignment is what `solution` is for, so this one generally
    /// breaks hard constraints — `score.hard` says by how many.
    ///
    /// It is a place to continue from, never an answer: a caller that reports it as a result
    /// reports something that breaks the rules. Set only while `solution` is `None`, so the two
    /// can never be confused; ask [`Self::reached`] for the best complete assignment regardless
    /// of how the run ended.
    ///
    /// Present on an [`SolveStatus::Infeasible`] outcome too, where it means the most useful
    /// thing a solver can say about an impossible model: this is as close as it gets.
    pub best_effort: Option<Solution>,
}

impl SolveOutcome {
    /// Builds a [`SolveStatus::Optimal`] outcome: `solution` is proven to have no better score.
    pub(crate) fn optimal(
        solution: Solution,
        statistics: SearchStatistics,
        bound: Option<HardSoftScore>,
    ) -> Self {
        Self {
            status: SolveStatus::Optimal,
            solution: Some(solution),
            statistics,
            bound,
            best_effort: None,
        }
    }

    /// Builds a [`SolveStatus::Feasible`] outcome: `solution` is the best incumbent found, not
    /// (yet, or ever) proven optimal.
    pub(crate) fn feasible(
        solution: Solution,
        statistics: SearchStatistics,
        bound: Option<HardSoftScore>,
    ) -> Self {
        Self {
            status: SolveStatus::Feasible,
            solution: Some(solution),
            statistics,
            bound,
            best_effort: None,
        }
    }

    /// Builds a [`SolveStatus::Infeasible`] outcome.
    pub(crate) fn infeasible(statistics: SearchStatistics) -> Self {
        Self {
            status: SolveStatus::Infeasible,
            solution: None,
            statistics,
            bound: None,
            best_effort: None,
        }
    }

    /// Builds a [`SolveStatus::Aborted`] outcome.
    pub(crate) fn aborted(reason: AbortReason, statistics: SearchStatistics) -> Self {
        Self {
            status: SolveStatus::Aborted(reason),
            solution: None,
            statistics,
            bound: None,
            best_effort: None,
        }
    }

    /// Attaches the best complete assignment this run reached (see [`Self::best_effort`]).
    ///
    /// Only an outcome without a solution has anything to attach: where a solution exists it is
    /// already the best complete assignment, and holding a second copy would invite a caller to
    /// pick the wrong one.
    pub(crate) fn with_best_effort(mut self, best_effort: Option<Solution>) -> Self {
        debug_assert!(
            self.solution.is_none(),
            "an outcome with a solution has no use for a fallback",
        );
        self.best_effort = best_effort;
        self
    }

    /// The best complete assignment this run reached, whether or not the search could vouch for
    /// it: the solution if there is one, the fallback otherwise.
    ///
    /// This is the question a repair search asks — it wants somewhere to continue from and does
    /// not care how the previous run ended. A caller deciding what to *report* asks for
    /// `solution` instead.
    pub fn reached(&self) -> Option<&Solution> {
        self.solution.as_ref().or(self.best_effort.as_ref())
    }
}

/// Checks whether a search run has exceeded its cancellation token, time limit, or node budget,
/// returning the specific reason so callers can report it via [`SolveStatus::Aborted`].
///
/// Shared by all search-based solvers (Backtracking, Branch & Bound, Local Search, LNS).
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn check_abort(
    options: &SolverOptions,
    start_time: Instant,
    step_count: u64,
) -> Option<AbortReason> {
    if let Some(token) = &options.cancellation_token
        && token.is_cancelled()
    {
        return Some(AbortReason::Cancelled);
    }
    if let Some(limit) = options.time_limit
        && start_time.elapsed() >= limit
    {
        return Some(AbortReason::Timeout);
    }
    if let Some(max_nodes) = options.max_nodes
        && step_count >= max_nodes
    {
        return Some(AbortReason::NodeLimit);
    }
    None
}

/// `dom/wdeg` variable-ordering heuristic: among unassigned variables, picks the one minimizing
/// `domain_size / weighted_degree`, where `weighted_degree` is the sum of `weights` (initialized
/// to 1, incremented by [`crate::propagation::engine::PropagationEngine::propagate`] each time a
/// constraint causes a conflict) over constraints connecting `var` to at least one other
/// still-unassigned variable. Constraints that repeatedly cause conflicts accumulate weight, so
/// the heuristic increasingly favors branching on the variables most involved in past failures —
/// "fail-first" driven by learned conflict history rather than domain size alone.
///
/// Shared by exact tree-search solvers ([`crate::solver::BacktrackingSolver`],
/// [`crate::solver::BranchAndBoundSolver`]).
///
/// Ties (equal ratio) resolve to the lowest [`VariableId`], not iteration order: `graph`
/// stores variables in a `HashMap`, whose iteration order is randomized per process, so
/// breaking ties by insertion/iteration order would make search behavior — and therefore any
/// benchmark — unreproducible from one run to the next despite identical input and options.
///
/// # Complexity
/// Time: O(N log N + N * D) where N is number of variables (the sort) and D is the max
/// constraint degree per variable. Space: O(N) for the sorted id list.
///
/// # References
/// Boussemart, F., Hemery, F., Lecoutre, C., & Sais, L. (2004). *Boosting systematic search by
/// weighting constraints*. ECAI 2004.
pub(crate) fn select_dom_wdeg_variable(
    graph: &ConstraintGraph,
    domains: &HashMap<VariableId, Domain>,
    assignment: &HashMap<VariableId, i64>,
    weights: &HashMap<ConstraintId, u32>,
) -> Option<VariableId> {
    let mut best_var = None;
    let mut best_ratio = f64::INFINITY;

    let mut var_ids: Vec<VariableId> = graph.variables().keys().copied().collect();
    var_ids.sort_unstable();

    for var_id in var_ids {
        if assignment.contains_key(&var_id) {
            continue;
        }
        let Some(domain) = domains.get(&var_id) else {
            continue;
        };
        let dom_size = domain.len().max(1) as f64;

        let mut weighted_degree: u64 = 0;
        for &cid in graph.constraints_for_variable(var_id) {
            if let Some(constraint) = graph.get_constraint(cid) {
                let connects_unassigned_other = constraint
                    .scope()
                    .iter()
                    .any(|&v| v != var_id && !assignment.contains_key(&v));
                if connects_unassigned_other {
                    weighted_degree += u64::from(*weights.get(&cid).unwrap_or(&1));
                }
            }
        }
        let ratio = dom_size / (weighted_degree.max(1) as f64);
        if ratio < best_ratio {
            best_ratio = ratio;
            best_var = Some(var_id);
        }
    }

    best_var
}

/// Least-constraining-value heuristic: orders `values` so that the value shared by the fewest
/// still-unassigned neighbors' domains comes first.
///
/// A neighbor is any other variable connected to `var_id` through at least one constraint,
/// counted once per connecting constraint (variables joined by several constraints weigh more,
/// mirroring how [`select_dom_wdeg_variable`] sums weight across constraints rather than
/// counting neighbors once). This is a cheap, domain-neutral proxy for "leaves the most room
/// for neighbors": it reads current domains rather than evaluating each constraint's semantics
/// per candidate value, which a full least-constraining-value pass would require (one trial
/// propagation per value).
///
/// # Complexity
/// Time: O(|values| * degree) where degree is the number of constraint-connected neighbor
/// occurrences. Space: O(|values|).
///
/// # References
/// Haralick, R. M., & Elliott, G. L. (1980). *Increasing tree search efficiency for constraint
/// satisfaction problems*. Artificial Intelligence, 14(3), 263-313.
pub(crate) fn order_values_by_neighbor_domain_size(
    graph: &ConstraintGraph,
    domains: &HashMap<VariableId, Domain>,
    assignment: &HashMap<VariableId, i64>,
    var_id: VariableId,
    values: Vec<i64>,
) -> Vec<i64> {
    let neighbors: Vec<VariableId> = graph
        .constraints_for_variable(var_id)
        .iter()
        .filter_map(|&cid| graph.get_constraint(cid))
        .flat_map(|constraint| constraint.scope().iter().copied())
        .filter(|&v| v != var_id && !assignment.contains_key(&v))
        .collect();

    let mut scored: Vec<(i64, usize)> = values
        .into_iter()
        .map(|val| {
            let shared = neighbors
                .iter()
                .filter(|&&n| domains.get(&n).is_some_and(|d| d.contains(val)))
                .count();
            (val, shared)
        })
        .collect();

    scored.sort_by_key(|&(_, shared)| shared);
    scored.into_iter().map(|(val, _)| val).collect()
}

/// One level of an explicit depth-first search stack: the variable branched on here, the values
/// still untried, and the trail position to undo to when the value in flight fails.
///
/// The searches keep their descent on a stack of these rather than on the call stack, because
/// the depth of an assignment search *is* the number of variables. A recursive descent therefore
/// overflows once an instance is large enough — and a stack overflow aborts the process, so the
/// caller gets no result at all where it should have got "no solution within the budget".
pub(crate) struct SearchFrame {
    variable: VariableId,
    values: Vec<i64>,
    next: usize,
    /// Trail position recorded before the value currently being tried; `None` while this frame
    /// has no value assigned — before its first attempt, and after each one is undone.
    checkpoint: Option<usize>,
}

impl SearchFrame {
    pub(crate) fn new(variable: VariableId, values: Vec<i64>) -> Self {
        Self {
            variable,
            values,
            next: 0,
            checkpoint: None,
        }
    }

    /// Undoes the value currently in flight, if there is one, leaving the frame ready for its
    /// next value. Only the domains touched since the checkpoint are restored.
    pub(crate) fn undo_attempt(
        &mut self,
        domains: &mut TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
    ) {
        if let Some(checkpoint) = self.checkpoint.take() {
            assignment.remove(&self.variable);
            domains.undo_to(checkpoint);
        }
    }

    /// Takes the next untried value and assigns it, returning the variable it was assigned to.
    /// `None` once the frame has tried everything — the caller then pops it and resumes the
    /// frame below.
    pub(crate) fn assign_next(
        &mut self,
        domains: &mut TrailedDomains,
        assignment: &mut HashMap<VariableId, i64>,
    ) -> Option<VariableId> {
        let value = *self.values.get(self.next)?;
        self.next += 1;
        // Taken before the assignment, so undoing returns to the parent's fixpoint.
        self.checkpoint = Some(domains.checkpoint());
        assignment.insert(self.variable, value);
        if let Some(domain) = domains.get_mut(&self.variable) {
            domain.assign(value);
        }
        Some(self.variable)
    }
}

/// Undoes every attempt still in flight, restoring `domains` and `assignment` to the state the
/// descent started from.
///
/// Callers pass their frames deepest-first. This is what a recursive descent did implicitly
/// while unwinding, and it is needed wherever a search is abandoned mid-tree — on a timeout or
/// a restart budget — so that "no result" does not also mean "left the domains half-pruned".
pub(crate) fn unwind<'a>(
    frames: impl IntoIterator<Item = &'a mut SearchFrame>,
    domains: &mut TrailedDomains,
    assignment: &mut HashMap<VariableId, i64>,
) {
    for frame in frames {
        frame.undo_attempt(domains, assignment);
    }
}
