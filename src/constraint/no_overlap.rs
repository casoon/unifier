//! Disjunctive / `NoOverlap` global constraint for unary resource scheduling.
//!
//! Enforces that no two intervals scheduled on the same unary resource overlap in time.
//!
//! References:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.
//! - Vilím, P. (2004). *O(n log n) filtering algorithms for unary resource constraint*. CPAIOR 2004, LNCS 3049.

use crate::constraint::{
    Constraint, PropagationResult, domain_bounds, duration_as_i64, energetic_overload, prune,
};
use crate::model::domain::TrailedDomains;
use crate::model::interval::Interval;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Pair of interval start and duration specifiers for non-overlapping execution.
#[derive(Debug, Clone)]
pub struct TaskInterval {
    pub start: VariableId,
    pub duration: u64,
}

/// Global `NoOverlap` constraint for a set of task intervals.
#[derive(Debug, Clone)]
pub struct NoOverlap {
    tasks: Vec<TaskInterval>,
    scope: Vec<VariableId>,
}

impl NoOverlap {
    /// Creates a `NoOverlap` constraint for the given intervals with fixed durations.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is number of intervals.
    pub fn new(tasks: Vec<TaskInterval>) -> Self {
        let scope = tasks.iter().map(|t| t.start).collect();
        Self { tasks, scope }
    }

    /// Creates a `NoOverlap` constraint from a slice of [`Interval`] objects with fixed durations.
    pub fn from_intervals(intervals: &[Interval], durations: &[u64]) -> Self {
        assert_eq!(intervals.len(), durations.len());
        let tasks = intervals
            .iter()
            .zip(durations.iter())
            .map(|(inv, &duration)| TaskInterval {
                start: inv.start(),
                duration,
            })
            .collect();
        Self::new(tasks)
    }
}

impl Constraint for NoOverlap {
    fn name(&self) -> &str {
        "NoOverlap"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        let n = self.tasks.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let t1 = &self.tasks[i];
                let t2 = &self.tasks[j];

                if let (Some(&s1), Some(&s2)) =
                    (assignment.get(&t1.start), assignment.get(&t2.start))
                {
                    let end1 = s1.saturating_add(duration_as_i64(t1.duration));
                    let end2 = s2.saturating_add(duration_as_i64(t2.duration));

                    // Overlap condition: not (end1 <= s2 || end2 <= s1)
                    if end1 > s2 && end2 > s1 {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let mut changed = false;
        let n = self.tasks.len();

        // Energetic-reasoning overload check (see `energetic_overload`'s doc comment): catches
        // infeasibilities that require reasoning about 3+ tasks together, which the pairwise
        // precedence pushing below cannot see. Unary resource, so capacity is 1 and each task's
        // "energy" is just its duration (implicit demand 1).
        let energy_windows: Vec<(i64, i64, i64)> = self
            .tasks
            .iter()
            .filter_map(|task| {
                let (min, max) = domain_bounds(domains, task.start)?;
                let lct = max.saturating_add(duration_as_i64(task.duration));
                Some((min, lct, duration_as_i64(task.duration)))
            })
            .collect();
        if energetic_overload(&energy_windows, 1) {
            return PropagationResult::Conflict;
        }

        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }

                let t1 = &self.tasks[i];
                let t2 = &self.tasks[j];

                let (min1, max1) = match domain_bounds(domains, t1.start) {
                    Some(bounds) => bounds,
                    None => continue,
                };

                let (min2, max2) = match domain_bounds(domains, t2.start) {
                    Some(bounds) => bounds,
                    None => continue,
                };

                let end1_min = min1.saturating_add(duration_as_i64(t1.duration));
                let end2_min = min2.saturating_add(duration_as_i64(t2.duration));

                // If t1 must end after t2 starts (end1_min > max2), then t1 must follow t2: start1 >= end2_min
                if end1_min > max2
                    && let Some(result) = prune(domains, &mut changed, t1.start, |d| {
                        d.remove_below(end2_min)
                    })
                {
                    return result;
                }

                // If t2 must end after t1 starts (end2_min > max1), then t2 must follow t1: start2 >= end1_min
                if end2_min > max1
                    && let Some(result) = prune(domains, &mut changed, t2.start, |d| {
                        d.remove_below(end1_min)
                    })
                {
                    return result;
                }
            }
        }

        PropagationResult::Success { changed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;

    /// Three tasks, duration 2 each, all with start in `[0,3]` (so `est=0`, `lct=3+2=5` for
    /// each): no *pair* overloads (any two need 4 time units, window is 5 — fits), but all three
    /// together need 6 time units in a window of 5 — infeasible only as a triple. Pairwise
    /// precedence pushing (the pre-existing propagation below the new check) cannot see this;
    /// `energetic_overload` must.
    #[test]
    fn test_propagate_detects_triple_overload_beyond_pairwise_reasoning() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        for &v in &[a, b, c] {
            domains.insert(v, Domain::range(0, 3));
        }
        let mut trailed = TrailedDomains::new(domains);

        let constraint = NoOverlap::new(vec![
            TaskInterval {
                start: a,
                duration: 2,
            },
            TaskInterval {
                start: b,
                duration: 2,
            },
            TaskInterval {
                start: c,
                duration: 2,
            },
        ]);

        assert_eq!(
            constraint.propagate(&mut trailed),
            PropagationResult::Conflict
        );
    }

    /// Same shape as above but with just enough room (`start` domain widened by 1): total energy
    /// (6) now exactly fits the window (6), so no conflict — confirms the check isn't
    /// over-eager/unsound.
    #[test]
    fn test_propagate_no_false_conflict_when_energy_exactly_fits() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        for &v in &[a, b, c] {
            domains.insert(v, Domain::range(0, 4)); // lct = 4+2=6, est=0, window=6, energy=6
        }
        let mut trailed = TrailedDomains::new(domains);

        let constraint = NoOverlap::new(vec![
            TaskInterval {
                start: a,
                duration: 2,
            },
            TaskInterval {
                start: b,
                duration: 2,
            },
            TaskInterval {
                start: c,
                duration: 2,
            },
        ]);

        assert_eq!(
            constraint.propagate(&mut trailed),
            PropagationResult::Success { changed: false }
        );
    }
}
