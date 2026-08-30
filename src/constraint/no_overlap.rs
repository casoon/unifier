//! Disjunctive / `NoOverlap` global constraint for unary resource scheduling.
//!
//! Enforces that no two intervals scheduled on the same unary resource overlap in time.
//!
//! References:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.
//! - Vilím, P. (2004). *O(n log n) filtering algorithms for unary resource constraint*. CPAIOR 2004, LNCS 3049.

use crate::constraint::{domain_bounds, prune, Constraint, PropagationResult};
use crate::model::domain::Domain;
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

                if let (Some(&s1), Some(&s2)) = (assignment.get(&t1.start), assignment.get(&t2.start)) {
                    let end1 = s1 + t1.duration as i64;
                    let end2 = s2 + t2.duration as i64;

                    // Overlap condition: not (end1 <= s2 || end2 <= s1)
                    if end1 > s2 && end2 > s1 {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;
        let n = self.tasks.len();

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

                let end1_min = min1 + t1.duration as i64;
                let end2_min = min2 + t2.duration as i64;

                // If t1 must end after t2 starts (end1_min > max2), then t1 must follow t2: start1 >= end2_min
                if end1_min > max2 {
                    if let Some(result) = prune(domains, &mut changed, t1.start, |d| d.remove_below(end2_min)) {
                        return result;
                    }
                }

                // If t2 must end after t1 starts (end2_min > max1), then t2 must follow t1: start2 >= end1_min
                if end2_min > max1 {
                    if let Some(result) = prune(domains, &mut changed, t2.start, |d| d.remove_below(end1_min)) {
                        return result;
                    }
                }
            }
        }

        PropagationResult::Success { changed }
    }
}
