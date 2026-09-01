//! Search cancellation tokens and runtime execution statistics.
//!
//! Provides thread-safe cancellation handles for interrupting search algorithms asynchronously,
//! as well as search statistics tracking node count and duration.
//!
//! References:
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.

use std::time::Duration;

/// Thread-safe cancellation handle allowing external interruption of running solvers.
///
/// Re-exported from `pathwise`: the same `Arc<AtomicBool>`-based, CSP-independent primitive is
/// shared with `pathwise`'s own `branch_and_bound`/`local_search`/`large_neighborhood_search`,
/// instead of `unifier` maintaining a duplicate implementation.
pub use pathwise::core::cancellation::CancellationToken;

/// Statistics collected during solver execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchStatistics {
    /// Number of search nodes expanded.
    pub nodes_expanded: u64,
    /// Total duration elapsed.
    pub elapsed: Duration,
}
