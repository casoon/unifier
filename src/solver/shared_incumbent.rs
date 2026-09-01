//! Thread-safe shared incumbent for portfolio search coordination.
//!
//! Lets independently-running solver workers (see [`crate::solver::ParallelSolver`]) both
//! contribute improving solutions and consult the best one found so far by *any* worker — e.g.
//! a fast, incomplete solver (Local Search, LNS) can hand Branch & Bound a strong starting bound
//! instead of it beginning from scratch.
//!
//! Reference:
//! - Gomes, C. P., & Selman, B. (2001). *Algorithm portfolios*. Artificial Intelligence, 126(1-2), 43-62.

use crate::model::variable::VariableId;
use crate::score::HardSoftScore;
use std::collections::HashMap;

/// A complete variable assignment paired with its score.
type ScoredAssignment = (HashMap<VariableId, i64>, HardSoftScore);

/// The best feasible `(assignment, score)` pair found so far across all portfolio workers
/// sharing this handle, if any. Cloning shares the same underlying state (see
/// [`crate::solver::CancellationToken`] for the identical pattern).
///
/// A thin wrapper around `pathwise`'s generic [`pathwise::core::incumbent::SharedIncumbent`],
/// specialized to `unifier`'s assignment representation — `unifier`'s public method
/// names/signatures are unchanged.
#[derive(Debug, Clone, Default)]
pub struct SharedIncumbent {
    inner: pathwise::core::incumbent::SharedIncumbent<HashMap<VariableId, i64>, HardSoftScore>,
}

impl SharedIncumbent {
    /// Creates an empty shared incumbent (no solution found yet).
    ///
    /// Time & Space: O(1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current best score, if any.
    ///
    /// # Complexity
    /// Time: O(1) (one mutex lock, no clone of the assignment).
    pub fn best_score(&self) -> Option<HardSoftScore> {
        self.inner.best_score()
    }

    /// Returns a clone of the current best `(assignment, score)` pair, if any.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is the assignment size, to clone it out from behind the lock.
    pub fn best(&self) -> Option<ScoredAssignment> {
        self.inner.best()
    }

    /// Replaces the incumbent with `(assignment, score)` if `score` improves on the current
    /// best (or none exists yet). Returns `true` if it did.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is the assignment size (cloned into shared storage on
    /// improvement; O(1) otherwise).
    pub fn offer(&self, assignment: &HashMap<VariableId, i64>, score: HardSoftScore) -> bool {
        self.inner.offer(assignment, score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(hard: i64, soft: i64) -> HardSoftScore {
        HardSoftScore::new(hard, soft)
    }

    #[test]
    fn test_offer_accepts_first_solution() {
        let incumbent = SharedIncumbent::new();
        let assignment: HashMap<VariableId, i64> = [(VariableId(0), 1)].into_iter().collect();
        assert!(incumbent.offer(&assignment, score(0, 5)));
        assert_eq!(incumbent.best_score(), Some(score(0, 5)));
    }

    #[test]
    fn test_offer_rejects_non_improving_solution() {
        let incumbent = SharedIncumbent::new();
        let assignment: HashMap<VariableId, i64> = [(VariableId(0), 1)].into_iter().collect();
        assert!(incumbent.offer(&assignment, score(0, 5)));
        assert!(!incumbent.offer(&assignment, score(0, 3)));
        assert_eq!(
            incumbent.best_score(),
            Some(score(0, 5)),
            "a worse offer must not overwrite the existing incumbent"
        );
    }

    #[test]
    fn test_offer_accepts_strictly_improving_solution() {
        let incumbent = SharedIncumbent::new();
        let assignment: HashMap<VariableId, i64> = [(VariableId(0), 1)].into_iter().collect();
        incumbent.offer(&assignment, score(0, 5));
        assert!(incumbent.offer(&assignment, score(0, 7)));
        assert_eq!(incumbent.best_score(), Some(score(0, 7)));
    }

    #[test]
    fn test_clone_shares_underlying_state() {
        let incumbent = SharedIncumbent::new();
        let handle = incumbent.clone();
        let assignment: HashMap<VariableId, i64> = [(VariableId(0), 1)].into_iter().collect();
        handle.offer(&assignment, score(0, 5));
        assert_eq!(
            incumbent.best_score(),
            Some(score(0, 5)),
            "clones must observe each other's updates"
        );
    }
}
