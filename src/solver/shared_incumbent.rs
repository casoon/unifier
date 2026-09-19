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

/// What portfolio workers sharing this handle know between them. Cloning shares the same
/// underlying state (see [`crate::solver::CancellationToken`] for the identical pattern).
///
/// Two things are held, because *what may be worked from* and *what may be handed back* are
/// different questions:
///
/// - [`Self::best`] — the best **feasible** solution. This is what a portfolio returns and what
///   Branch & Bound bounds against, so nothing that breaks a hard constraint may enter it.
/// - [`Self::center`] — the best complete assignment of any kind, feasible or not. A repair
///   search needs somewhere to start, and an assignment four constraints short of a schedule is
///   an excellent place; it is simply not an answer.
///
/// A single slot can only be one of the two. It used to be the first, so every worker filtered
/// its offers down to feasible ones and a near miss was dropped on the floor — leaving the one
/// solver able to repair it with nothing to repair. Callers now offer whatever they have and
/// this type decides where it belongs.
///
/// The distinction only matters before anything feasible exists: a feasible score outranks every
/// infeasible one, so from the first real solution onward both slots hold it.
///
/// Built on `pathwise`'s generic [`pathwise::core::incumbent::SharedIncumbent`], specialized to
/// `unifier`'s assignment representation.
#[derive(Debug, Clone, Default)]
pub struct SharedIncumbent {
    feasible: pathwise::core::incumbent::SharedIncumbent<HashMap<VariableId, i64>, HardSoftScore>,
    center: pathwise::core::incumbent::SharedIncumbent<HashMap<VariableId, i64>, HardSoftScore>,
}

impl SharedIncumbent {
    /// Creates an empty shared incumbent (nothing found yet).
    ///
    /// Time & Space: O(1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the best feasible score, if any.
    ///
    /// # Complexity
    /// Time: O(1) (one mutex lock, no clone of the assignment).
    pub fn best_score(&self) -> Option<HardSoftScore> {
        self.feasible.best_score()
    }

    /// Returns a clone of the best **feasible** `(assignment, score)` pair, if any. Never
    /// returns an assignment that breaks a hard constraint.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is the assignment size, to clone it out from behind the lock.
    pub fn best(&self) -> Option<ScoredAssignment> {
        self.feasible.best()
    }

    /// Returns a clone of the best complete assignment offered so far, **whether or not it is
    /// feasible** — the best known starting point for a repair search.
    ///
    /// Read this to decide where to work; read [`Self::best`] to decide what to report.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is the assignment size, to clone it out from behind the lock.
    pub fn center(&self) -> Option<ScoredAssignment> {
        self.center.best()
    }

    /// Records `(assignment, score)` wherever it belongs: always a candidate starting point, and
    /// a candidate answer too when it is feasible. Returns `true` if anything improved.
    ///
    /// Callers pass what they have and do not pre-filter. An infeasible offer can only ever
    /// reach [`Self::center`].
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is the assignment size (cloned into shared storage on
    /// improvement; O(1) otherwise).
    pub fn offer(&self, assignment: &HashMap<VariableId, i64>, score: HardSoftScore) -> bool {
        let improved_center = self.center.offer(assignment, score);
        if score.is_feasible() {
            // A feasible score outranks every infeasible one, so this improves the center too
            // whenever it improves the best — the two cannot disagree in the other direction.
            return self.feasible.offer(assignment, score) || improved_center;
        }
        improved_center
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(hard: i64, soft: i64) -> HardSoftScore {
        HardSoftScore::new(hard, soft)
    }

    /// The whole point of the split: an assignment that still breaks hard constraints is a
    /// place to work from and never an answer. Before the split a single slot had to be one or
    /// the other, every worker filtered its offers down to feasible ones, and the near miss that
    /// a repair search wants was dropped on the floor.
    #[test]
    fn an_infeasible_offer_is_a_starting_point_and_never_an_answer() {
        let incumbent = SharedIncumbent::new();
        let assignment: HashMap<VariableId, i64> = [(VariableId(0), 1)].into_iter().collect();

        assert!(incumbent.offer(&assignment, score(-4, 0)));
        assert_eq!(
            incumbent.center().map(|(_, score)| score),
            Some(score(-4, 0))
        );
        assert_eq!(
            incumbent.best(),
            None,
            "an answer must hold every hard rule"
        );
        assert_eq!(incumbent.best_score(), None);

        // A near miss that is less near does not displace the better one.
        let worse: HashMap<VariableId, i64> = [(VariableId(0), 2)].into_iter().collect();
        assert!(!incumbent.offer(&worse, score(-9, 0)));
        assert_eq!(
            incumbent.center().map(|(_, score)| score),
            Some(score(-4, 0))
        );

        // The first feasible solution takes both slots: it outranks every infeasible one.
        let solved: HashMap<VariableId, i64> = [(VariableId(0), 3)].into_iter().collect();
        assert!(incumbent.offer(&solved, score(0, -2)));
        assert_eq!(incumbent.best().map(|(_, score)| score), Some(score(0, -2)));
        assert_eq!(
            incumbent.center().map(|(_, score)| score),
            Some(score(0, -2))
        );
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
