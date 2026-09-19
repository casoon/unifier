//! `Cumulative` constraint for multi-capacity resource scheduling.
//!
//! Ensures that at any point in time, the sum of resource demands of active tasks
//! does not exceed the total available resource capacity.
//!
//! References:
//! - Aggoun, A., & Beldiceanu, N. (1993). *Extending CHIP in order to solve cumulative scheduling problems*.
//!   Mathematical and Computer Modelling, 17(7), 57-73.
//! - Wolf, A. (2003). *Pruning Algorithms for the Cumulative Constraint*. Workshop on Constraint Solving.

use crate::constraint::{
    Assignment, Constraint, Explanation, PropagationResult, domain_bounds, duration_as_i64,
    energetic_overload, prune,
};
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// Specification of a task participating in a [`Cumulative`] constraint.
#[derive(Debug, Clone)]
pub struct TaskDemand {
    pub start: VariableId,
    pub duration: u64,
    pub demand: u32,
}

/// Global `Cumulative` constraint over tasks sharing a cumulative resource.
#[derive(Debug, Clone)]
pub struct Cumulative {
    tasks: Vec<TaskDemand>,
    capacity: u32,
    scope: Vec<VariableId>,
}

impl Cumulative {
    /// Creates a new `Cumulative` constraint.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is number of tasks.
    pub fn new(tasks: Vec<TaskDemand>, capacity: u32) -> Self {
        let scope = tasks.iter().map(|t| t.start).collect();
        Self {
            tasks,
            capacity,
            scope,
        }
    }

    /// Returns the capacity threshold of the cumulative resource.
    ///
    /// Time complexity: O(1).
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

impl Constraint for Cumulative {
    fn name(&self) -> &str {
        "Cumulative"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    /// The total overload: how much demand exceeds the capacity, summed over the moments where
    /// it does. Removing one task from an overloaded stretch therefore registers even while the
    /// stretch is still over — a yes/no answer would hide every step but the last.
    fn violations(&self, assignment: &HashMap<VariableId, i64>) -> u32 {
        let mut time_points = Vec::new();
        for task in &self.tasks {
            if let Some(&start) = assignment.get(&task.start) {
                time_points.push(start);
                time_points.push(start.saturating_add(duration_as_i64(task.duration)));
            }
        }
        time_points.sort_unstable();
        time_points.dedup();

        let mut overload = 0u32;
        for &moment in &time_points {
            let mut demand: u32 = 0;
            for task in &self.tasks {
                if let Some(&start) = assignment.get(&task.start) {
                    let end = start.saturating_add(duration_as_i64(task.duration));
                    if moment >= start && moment < end {
                        demand = demand.saturating_add(task.demand);
                    }
                }
            }
            overload = overload.saturating_add(demand.saturating_sub(self.capacity));
        }
        overload
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        // Collect all potential time boundaries
        let mut time_points = Vec::new();
        for task in &self.tasks {
            if let Some(&s) = assignment.get(&task.start) {
                time_points.push(s);
                time_points.push(s.saturating_add(duration_as_i64(task.duration)));
            }
        }
        time_points.sort_unstable();
        time_points.dedup();

        // Check demand at each active interval
        for &t in &time_points {
            let mut total_demand: u32 = 0;
            for task in &self.tasks {
                if let Some(&s) = assignment.get(&task.start) {
                    let end = s.saturating_add(duration_as_i64(task.duration));
                    if t >= s && t < end {
                        total_demand = total_demand.saturating_add(task.demand);
                    }
                }
            }
            if total_demand > self.capacity {
                return false;
            }
        }

        true
    }

    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        let mut time_points: Vec<i64> = self
            .tasks
            .iter()
            .filter_map(|task| assignment.get(&task.start).copied())
            .collect();
        time_points.sort_unstable();
        time_points.dedup();

        for time in time_points {
            let active: Vec<&TaskDemand> = self
                .tasks
                .iter()
                .filter(|task| {
                    assignment.get(&task.start).is_some_and(|&start| {
                        time >= start && time < start.saturating_add(duration_as_i64(task.duration))
                    })
                })
                .collect();
            let demand = active
                .iter()
                .fold(0u32, |total, task| total.saturating_add(task.demand));
            if demand > self.capacity {
                return Some(Explanation {
                    constraint_name: "Cumulative",
                    involved: active.iter().map(|task| task.start).collect(),
                    message: format!(
                        "Resource demand {demand} at time {time} exceeds capacity {}",
                        self.capacity
                    ),
                });
            }
        }
        None
    }

    fn validate(&self) -> Result<(), String> {
        for task in &self.tasks {
            if task.demand > self.capacity {
                return Err(format!(
                    "task on {:?} has demand {} exceeding capacity {}: can never be scheduled",
                    task.start, task.demand, self.capacity
                ));
            }
        }
        Ok(())
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let mut changed = false;

        // Safety net for graphs built without `ConstraintGraph::validate` (which already rejects
        // this structurally, see `validate` above).
        for task in &self.tasks {
            if task.demand > self.capacity {
                return PropagationResult::Conflict;
            }
        }

        // Energetic-reasoning overload check (see `energetic_overload`'s doc comment): catches
        // infeasibilities that require reasoning about 3+ tasks' combined demand, which the
        // pairwise mandatory-part reasoning below cannot see.
        let energy_windows: Vec<(i64, i64, i64)> = self
            .tasks
            .iter()
            .filter_map(|task| {
                let (min, max) = domain_bounds(domains, task.start)?;
                let lct = max.saturating_add(duration_as_i64(task.duration));
                let energy = i64::from(task.demand).saturating_mul(duration_as_i64(task.duration));
                Some((min, lct, energy))
            })
            .collect();
        if energetic_overload(&energy_windows, self.capacity) {
            return PropagationResult::Conflict;
        }

        // For each task, check if scheduling it at current min would overlap with mandatory parts of other tasks
        // exceeding capacity
        for i in 0..self.tasks.len() {
            let t1 = &self.tasks[i];
            let (min1, max1) = match domain_bounds(domains, t1.start) {
                Some(bounds) => bounds,
                None => continue,
            };

            // Calculate mandatory parts of all other tasks
            // Mandatory part of task j exists if min_j + duration_j > max_j
            for t_check in min1..=max1 {
                let mut total_demand = t1.demand;
                for (j, t2) in self.tasks.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    if let Some(d2) = domains.get(&t2.start)
                        && let (Some(min2), Some(max2)) = (d2.min(), d2.max())
                    {
                        let mand_start = max2;
                        let mand_end = min2.saturating_add(duration_as_i64(t2.duration));
                        if mand_start < mand_end && t_check >= mand_start && t_check < mand_end {
                            total_demand = total_demand.saturating_add(t2.demand);
                        }
                    }
                }

                if total_demand > self.capacity {
                    // t_check is infeasible for t1.start -> remove t_check from t1 domain
                    if let Some(result) =
                        prune(domains, &mut changed, t1.start, |d| d.remove(t_check))
                    {
                        return result;
                    }
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
    use crate::model::variable::VariableId;

    /// Capacity 2, three tasks demand 2 / duration 2 each, all start in `[0,3]` (`est=0`,
    /// `lct=3+2=5`). Energy per task = demand*duration = 4, sum = 12; capacity*window = 2*5 = 10.
    /// 12 > 10 -> infeasible as a triple even though no pair overloads (any two: 8 <= 10) and no
    /// single point's mandatory-part demand exceeds capacity (durations don't force an overlap at
    /// any one instant given the domain width) — only energetic reasoning over all three sees it.
    #[test]
    fn test_propagate_detects_triple_energy_overload_beyond_mandatory_parts() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        for &v in &[a, b, c] {
            domains.insert(v, Domain::range(0, 3));
        }
        let mut trailed = TrailedDomains::new(domains);

        let constraint = Cumulative::new(
            vec![
                TaskDemand {
                    start: a,
                    duration: 2,
                    demand: 2,
                },
                TaskDemand {
                    start: b,
                    duration: 2,
                    demand: 2,
                },
                TaskDemand {
                    start: c,
                    duration: 2,
                    demand: 2,
                },
            ],
            2,
        );

        assert_eq!(
            constraint.propagate(&mut trailed),
            PropagationResult::Conflict
        );
    }

    /// Same triple, but capacity 2 with demand 1 each (energy sum 6 <= capacity*window 10): the
    /// extra concurrency headroom must NOT trigger a false conflict.
    #[test]
    fn test_propagate_no_false_conflict_with_enough_capacity() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        for &v in &[a, b, c] {
            domains.insert(v, Domain::range(0, 3));
        }
        let mut trailed = TrailedDomains::new(domains);

        let constraint = Cumulative::new(
            vec![
                TaskDemand {
                    start: a,
                    duration: 2,
                    demand: 1,
                },
                TaskDemand {
                    start: b,
                    duration: 2,
                    demand: 1,
                },
                TaskDemand {
                    start: c,
                    duration: 2,
                    demand: 1,
                },
            ],
            2,
        );

        assert_eq!(
            constraint.propagate(&mut trailed),
            PropagationResult::Success { changed: false }
        );
    }
}
