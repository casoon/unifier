//! Search cancellation tokens and runtime execution statistics.
//!
//! Provides thread-safe cancellation handles for interrupting search algorithms asynchronously,
//! as well as search statistics tracking node count and duration.
//!
//! References:
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Thread-safe cancellation handle allowing external interruption of running solvers.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a new inactive cancellation token.
    ///
    /// Time & Space: O(1).
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Triggers cancellation, signaling all solvers holding this token to abort search.
    ///
    /// Time complexity: O(1).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if cancellation has been requested.
    ///
    /// Time complexity: O(1).
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// Statistics collected during solver execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchStatistics {
    /// Number of search nodes expanded.
    pub nodes_expanded: u64,
    /// Total duration elapsed.
    pub elapsed: Duration,
}
