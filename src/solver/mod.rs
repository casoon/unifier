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

use crate::model::variable::VariableId;
use crate::score::HardSoftScore;
use std::collections::HashMap;
use std::time::Duration;

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
