//! Constraint satisfaction and optimization solvers.
//!
//! Provides Backtracking with MRV heuristics, Branch & Bound, Local Search, LNS, and Parallel Portfolio search.

pub mod backtracking;
pub mod branch_and_bound;
pub mod cancellation;
pub mod local_search;
pub mod lns;
pub mod parallel;
pub mod pathwise_bridge;

pub use backtracking::BacktrackingSolver;
pub use branch_and_bound::BranchAndBoundSolver;
pub use cancellation::{CancellationToken, SearchStatistics};
pub use local_search::LocalSearchSolver;
pub use lns::LnsSolver;
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

/// Result returned by a solver run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveResult {
    /// A valid assignment satisfying all hard constraints was found.
    Feasible {
        assignment: HashMap<VariableId, i64>,
        score: HardSoftScore,
    },
    /// The constraint network is provably unsatisfiable.
    Infeasible,
    /// Solver stopped due to time limit or node limit.
    Timeout,
}

/// Checks whether a search run has exceeded its cancellation token, time limit, or node budget.
///
/// Shared by all search-based solvers (Backtracking, Branch & Bound, Local Search, LNS).
///
/// # Complexity
/// Time & Space: O(1).
pub(crate) fn is_timed_out(options: &SolverOptions, start_time: Instant, step_count: u64) -> bool {
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
        if step_count >= max_nodes {
            return true;
        }
    }
    false
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
