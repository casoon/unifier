//! `Cumulative` constraint for multi-capacity resource scheduling.
//!
//! Ensures that at any point in time, the sum of resource demands of active tasks
//! does not exceed the total available resource capacity.
//!
//! References:
//! - Aggoun, A., & Beldiceanu, N. (1993). *Extending CHIP in order to solve cumulative scheduling problems*.
//!   Mathematical and Computer Modelling, 17(7), 57-73.
//! - Wolf, A. (2003). *Pruning Algorithms for the Cumulative Constraint*. Workshop on Constraint Solving.

use crate::constraint::{Constraint, PropagationResult, domain_bounds, duration_as_i64, prune};
use crate::model::domain::Domain;
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

    fn propagate(&self, domains: &mut HashMap<VariableId, Domain>) -> PropagationResult {
        let mut changed = false;

        // Safety net for graphs built without `ConstraintGraph::validate` (which already rejects
        // this structurally, see `validate` above).
        for task in &self.tasks {
            if task.demand > self.capacity {
                return PropagationResult::Conflict;
            }
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
