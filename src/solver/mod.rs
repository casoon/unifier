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
use crate::propagation::graph::ConstraintGraph;
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

/// Result returned by a solver run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveResult {
    /// A valid assignment satisfying all hard constraints was found.
    ///
    /// `proven_optimal` is `true` only if the solver established that no better `score` is
    /// reachable (e.g. [`BranchAndBoundSolver`] exhausted or bound-pruned the full search
    /// space). It is always `false` for solvers that cannot make that guarantee (Backtracking
    /// stops at the first feasible assignment; Local Search and LNS are incomplete heuristics) —
    /// in that case `assignment` is the best incumbent found before the search ended.
    Feasible {
        assignment: HashMap<VariableId, i64>,
        score: HardSoftScore,
        proven_optimal: bool,
    },
    /// The constraint network is provably unsatisfiable: the full search space was exhausted
    /// without finding any feasible assignment.
    Infeasible,
    /// The search stopped (see `reason`) before finding a feasible assignment and before the
    /// search space was exhausted, so neither feasibility nor infeasibility could be established.
    Aborted { reason: AbortReason },
}

/// Checks whether a search run has exceeded its cancellation token, time limit, or node budget,
/// returning the specific reason so callers can report it via [`SolveResult::Aborted`].
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
