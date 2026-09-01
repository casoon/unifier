//! Constraint satisfaction and optimization solvers.
//!
//! Provides Backtracking with MRV heuristics, Branch & Bound, Local Search, LNS, and Parallel Portfolio search.

pub mod backtracking;
pub mod branch_and_bound;
pub mod cancellation;
pub mod lns;
pub mod local_search;
pub mod parallel;
pub mod pathwise_bridge;

pub use backtracking::BacktrackingSolver;
pub use branch_and_bound::BranchAndBoundSolver;
pub use cancellation::{CancellationToken, SearchStatistics};
pub use lns::LnsSolver;
pub use local_search::LocalSearchSolver;
pub use parallel::ParallelSolver;
pub use pathwise_bridge::UnifierProblemAdapter;

use crate::model::domain::Domain;
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
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            time_limit: Some(Duration::from_secs(10)),
            max_nodes: None,
            cancellation_token: None,
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
        }
    }

    /// Builds a [`SolveStatus::Infeasible`] outcome.
    pub(crate) fn infeasible(statistics: SearchStatistics) -> Self {
        Self {
            status: SolveStatus::Infeasible,
            solution: None,
            statistics,
            bound: None,
        }
    }

    /// Builds a [`SolveStatus::Aborted`] outcome.
    pub(crate) fn aborted(reason: AbortReason, statistics: SearchStatistics) -> Self {
        Self {
            status: SolveStatus::Aborted(reason),
            solution: None,
            statistics,
            bound: None,
        }
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

/// Minimum Remaining Values (MRV / Fail-First) heuristic selecting the unassigned variable
/// with the smallest domain.
///
/// Shared by exact tree-search solvers (Backtracking, Branch & Bound).
///
/// # Complexity
/// Time: O(N) where N is number of variables. Space: O(1).
pub(crate) fn select_mrv_variable(
    graph: &ConstraintGraph,
    domains: &HashMap<VariableId, Domain>,
    assignment: &HashMap<VariableId, i64>,
) -> Option<VariableId> {
    let mut best_var = None;
    let mut min_domain_size = usize::MAX;

    for &var_id in graph.variables().keys() {
        if !assignment.contains_key(&var_id)
            && let Some(domain) = domains.get(&var_id)
        {
            let len = domain.len();
            if len < min_domain_size {
                min_domain_size = len;
                best_var = Some(var_id);
            }
        }
    }

    best_var
}

/// `dom/wdeg` variable-ordering heuristic: among unassigned variables, picks the one minimizing
/// `domain_size / weighted_degree`, where `weighted_degree` is the sum of `weights` (initialized
/// to 1, incremented by [`crate::propagation::engine::PropagationEngine::propagate`] each time a
/// constraint causes a conflict) over constraints connecting `var` to at least one other
/// still-unassigned variable. Constraints that repeatedly cause conflicts accumulate weight, so
/// the heuristic increasingly favors branching on the variables most involved in past failures —
/// "fail-first" driven by learned conflict history rather than domain size alone.
///
/// Currently used only by [`crate::solver::BacktrackingSolver`] (see
/// `plan/11-search-heuristics-and-global-constraints.md`, part A).
///
/// # Complexity
/// Time: O(N * D) where N is number of variables and D is the max constraint degree per
/// variable. Space: O(1) beyond the caller-owned `weights` map.
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

    for &var_id in graph.variables().keys() {
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
