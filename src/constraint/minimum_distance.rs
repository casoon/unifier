//! Symmetric minimum-distance constraint between two assigned values.
//!
//! Enforces `|value(first) - value(second)| >= min_distance`: the two decision variables must be
//! separated by at least `min_distance`. This is the generic primitive behind a person- or
//! resource-specific minimum break; the caller decides which pairs of assignments are *relevant*
//! (e.g. two lessons of the same participant) and what distance the variable values express. It is
//! deliberately independent of any calendar/break vocabulary, so a fixed break window
//! (`BreakTemplate`) stays a separate concept.
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann. (Binary temporal constraints.)

use crate::constraint::{Constraint, PropagationResult, compare_assigned, prune};
use crate::model::domain::{Domain, TrailedDomains};
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Constraint enforcing `|first - second| >= min_distance`.
#[derive(Debug, Clone)]
pub struct MinimumDistance {
    first: VariableId,
    second: VariableId,
    min_distance: i64,
    scope: [VariableId; 2],
}

impl MinimumDistance {
    /// Creates a constraint enforcing `|first - second| >= min_distance`.
    ///
    /// A non-positive `min_distance` is trivially satisfied (`|d| >= 0`).
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(first: VariableId, second: VariableId, min_distance: i64) -> Self {
        Self {
            first,
            second,
            min_distance,
            scope: [first, second],
        }
    }

    /// Returns the required minimum distance.
    ///
    /// Time complexity: O(1).
    pub const fn min_distance(&self) -> i64 {
        self.min_distance
    }

    /// The forbidden band `[w - d + 1, w + d - 1]` around a fixed value `w`: every value strictly
    /// closer than `d` to `w`. Empty when `d <= 0`.
    fn forbidden_band(&self, w: i64) -> (i64, i64) {
        (
            w.saturating_sub(self.min_distance).saturating_add(1),
            w.saturating_add(self.min_distance).saturating_sub(1),
        )
    }
}

impl Constraint for MinimumDistance {
    fn name(&self) -> &str {
        "MinimumDistance"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        compare_assigned(assignment, self.first, self.second, |first, second| {
            if self.min_distance <= 0 {
                return true;
            }
            first.abs_diff(second) >= self.min_distance as u64
        })
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let mut changed = false;

        // If one side is already fixed, prune the values too close to it from the other side.
        if let Some(fixed) = single_value(domains, self.first) {
            let (low, high) = self.forbidden_band(fixed);
            if let Some(result) = prune(domains, &mut changed, self.second, |domain| {
                remove_band(domain, low, high)
            }) {
                return result;
            }
        }
        if let Some(fixed) = single_value(domains, self.second) {
            let (low, high) = self.forbidden_band(fixed);
            if let Some(result) = prune(domains, &mut changed, self.first, |domain| {
                remove_band(domain, low, high)
            }) {
                return result;
            }
        }

        PropagationResult::Success { changed }
    }
}

/// Returns the single value of `var`'s domain, or `None` if unassigned/untracked.
fn single_value(domains: &TrailedDomains, var: VariableId) -> Option<i64> {
    domains
        .get(&var)
        .filter(|d| d.len() == 1)
        .and_then(Domain::min)
}

/// Removes every value of `domain` inside the inclusive band `[low, high]`.
///
/// A `Range` domain can only drop a prefix, a suffix, or become empty; a forbidden band strictly
/// inside it is left untouched (sound, just weaker). An `Explicit` domain drops the exact values.
fn remove_band(domain: &mut Domain, low: i64, high: i64) -> bool {
    if low > high {
        return false;
    }
    enum Plan {
        None,
        Empty,
        Below(i64),
        Above(i64),
        Values(Vec<i64>),
    }
    let plan = match &*domain {
        Domain::Range { min, max } => {
            let (min, max) = (*min, *max);
            if min > max {
                Plan::None
            } else if low <= min && max <= high {
                Plan::Empty
            } else if low <= min && min <= high {
                Plan::Below(high)
            } else if low <= max && max <= high {
                Plan::Above(low)
            } else {
                Plan::None
            }
        }
        Domain::Explicit(set) => Plan::Values(
            set.iter()
                .copied()
                .filter(|value| low <= *value && *value <= high)
                .collect(),
        ),
    };
    match plan {
        Plan::None => false,
        Plan::Empty => {
            *domain = Domain::from_values(std::iter::empty());
            true
        }
        // `Below(high)`/`Above(low)` imply `max > high`/`min < low` respectively (otherwise the
        // `Empty` arm would have matched), so neither bound saturates at an extreme.
        Plan::Below(high) => domain.remove_below(high.saturating_add(1)),
        Plan::Above(low) => domain.remove_above(low.saturating_sub(1)),
        Plan::Values(values) => {
            let mut changed = false;
            for value in values {
                changed |= domain.remove(value);
            }
            changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;

    fn assignment(entries: &[(VariableId, i64)]) -> HashMap<VariableId, i64> {
        entries.iter().copied().collect()
    }

    /// A distance exactly equal to the minimum is allowed.
    #[test]
    fn distance_exactly_at_the_minimum_is_allowed() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 3);
        assert!(constraint.is_satisfied(&assignment(&[(a, 0), (b, 3)])));
        assert!(constraint.is_satisfied(&assignment(&[(a, 3), (b, 0)])));
    }

    /// A distance shorter than the minimum is rejected, in either direction.
    #[test]
    fn too_short_distance_is_rejected() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 3);
        assert!(!constraint.is_satisfied(&assignment(&[(a, 1), (b, 2)])));
        assert!(!constraint.is_satisfied(&assignment(&[(a, 2), (b, 1)])));
        assert!(
            !constraint.is_satisfied(&assignment(&[(a, 4), (b, 4)])),
            "identical values have distance 0"
        );
    }

    /// A partial assignment (one side unassigned) is never a violation.
    #[test]
    fn partial_assignment_is_not_a_violation() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 3);
        assert!(constraint.is_satisfied(&assignment(&[(a, 0)])));
    }

    /// Propagation removes values too close to a fixed counterpart, but only from the removable
    /// ends of a range domain.
    #[test]
    fn propagation_prunes_values_near_a_fixed_counterpart() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 3);

        // `a` fixed at 5, `b` free in [5, 10]: the forbidden band around 5 is [3, 7], which covers
        // `b`'s lower end, so `b` is pruned up to [8, 10].
        let mut domains = TrailedDomains::new(HashMap::from([
            (a, Domain::from_values([5])),
            (b, Domain::range(5, 10)),
        ]));
        let result = constraint.propagate(&mut domains);
        assert!(matches!(
            result,
            PropagationResult::Success { changed: true }
        ));
        assert_eq!(domains.get(&b).unwrap().values(), vec![8, 9, 10]);
    }

    /// If the whole remaining domain is forbidden, propagation reports a conflict.
    #[test]
    fn propagation_reports_conflict_when_everything_is_too_close() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 3);
        let mut domains = TrailedDomains::new(HashMap::from([
            (a, Domain::from_values([5])),
            (b, Domain::range(4, 6)),
        ]));
        assert_eq!(
            constraint.propagate(&mut domains),
            PropagationResult::Conflict
        );
    }

    /// A non-positive minimum is trivially satisfied and never prunes.
    #[test]
    fn non_positive_minimum_is_trivial() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MinimumDistance::new(a, b, 0);
        assert!(constraint.is_satisfied(&assignment(&[(a, 7), (b, 7)])));
        let mut domains = TrailedDomains::new(HashMap::from([
            (a, Domain::from_values([7])),
            (b, Domain::from_values([7])),
        ]));
        assert_eq!(
            constraint.propagate(&mut domains),
            PropagationResult::Success { changed: false }
        );
    }

    /// End-to-end through the DSL and solver: a free activity is pushed to satisfy the distance.
    #[test]
    fn builder_and_solver_respect_the_minimum_distance() {
        use crate::dsl::ModelBuilder;
        use crate::solver::{BacktrackingSolver, SolverOptions};

        let mut builder = ModelBuilder::new();
        let pinned = builder.new_var("pinned", 0..=0);
        let free = builder.new_var("free", 0..=2);
        builder.add_minimum_distance(pinned, free, 2);
        let graph = builder.build().expect("model should validate");
        let solution = BacktrackingSolver::new()
            .solve(&graph, &SolverOptions::default())
            .solution
            .expect("feasible: free can move to distance 2");
        assert_eq!(solution.assignment[&free], 2);
    }

    /// End-to-end: no placement is far enough -> infeasible.
    #[test]
    fn builder_and_solver_report_infeasibility() {
        use crate::dsl::ModelBuilder;
        use crate::solver::{BacktrackingSolver, SolverOptions};

        let mut builder = ModelBuilder::new();
        let a = builder.new_var("a", 0..=1);
        let b = builder.new_var("b", 0..=1);
        builder.add_minimum_distance(a, b, 2);
        let graph = builder.build().expect("model should validate");
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        assert!(outcome.solution.is_none());
    }
}
