//! Disjunctive / `NoOverlap` global constraint for unary resource scheduling.
//!
//! Enforces that no two intervals scheduled on the same unary resource overlap in time.
//! Propagation combines pairwise precedence pushing, an energetic-reasoning overload check
//! (`energetic_overload`, detection only), and an edge-finding bound update
//! (`edge_finding_bound_updates`) that reasons about *sets* of tasks rather than pairs — see
//! their doc comments for the algorithms and soundness arguments. This is a straightforward
//! O(N^3) enumeration, not the O(N log N) Theta-tree the Vilím (2004) reference below describes;
//! see `plan/11-search-heuristics-and-global-constraints.md`, part D, for the trade-off.
//!
//! References:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.
//! - Vilím, P. (2004). *O(n log n) filtering algorithms for unary resource constraint*. CPAIOR 2004, LNCS 3049.
//! - Carlier, J., & Pinson, E. (1989). *An algorithm for solving the job-shop problem*. Management Science, 35(2), 164-176.

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

/// Edge-finding bound update for a unary resource: for each task `i`, finds the tightest sound
/// lower bound on its start implied by some other task set `Omega` (`i` not in `Omega`) that
/// would overload if `i` were scheduled to start before `Omega` finishes.
///
/// `tasks` gives each task's `(est, lct, duration)`, `None` for a task whose domain bounds are
/// unavailable (untracked variable or already-empty domain — excluded from every candidate `Omega`
/// and never itself updated, but its index is preserved so the result's indices still line up
/// with the caller's task list). Returns `(index, new_est)` pairs for every task whose bound can
/// be raised above its current `est`.
///
/// # Theorem (edge-finding bound update, unary resource)
/// For a task set `Omega` (task `i` not in `Omega`) with `p(Omega) > 0`, individually feasible
/// (`p(Omega) <= lct(Omega) - est(Omega)`, else the whole constraint is already `Conflict` — see
/// `energetic_overload`, checked before this runs): if
///
/// `min(est(Omega), est(i)) + p(Omega) + p(i) > lct(Omega)`
///
/// then in every valid schedule, `i` starts no earlier than `est(Omega) + p(Omega)`.
///
/// This is the classical result (Carlier, J., & Pinson, E. (1989). *An algorithm for solving the
/// job-shop problem*. Management Science, 35(2), 164-176; see also Baptiste, Le Pape, & Nuijten
/// (2001), *Constraint-Based Scheduling*, Springer). It was independently re-derived and verified
/// by direct proof (not merely transcribed) before this implementation, given the correctness
/// stakes of an unsound scheduling propagator — the full proof (including the case where `i`'s
/// own `lct` exceeds `lct(Omega)`, the subtle part naive derivations tend to get wrong) is
/// recorded in `plan/00-STATUS.md` / `plan/11-search-heuristics-and-global-constraints.md` rather
/// than reproduced here. [`crate::constraint::energetic_overload`] is the analogous but
/// *detection-only* (no bound update) technique this crate uses for `Cumulative` — the
/// multi-capacity generalization of this update rule is genuinely more involved (concurrent
/// tasks mean "i must follow Omega" isn't implied by overload alone) and was deliberately not
/// attempted here; see part D's "Ergebnis" section for the reasoning.
///
/// Candidate `Omega` sets are enumerated the same way as `energetic_overload`: for each task `i`,
/// for each candidate `lct` threshold `b` (drawn from the other tasks' actual `lct` values,
/// since a tighter bound can only ever be achieved at an actual task edge), `Omega` is every
/// other task with `lct <= b`.
///
/// # Complexity
/// Time: O(N^2) candidate `(i, b)` pairs x O(N) to build `Omega` and check -> O(N^3). Space:
/// O(N) for the result.
fn edge_finding_bound_updates(tasks: &[Option<(i64, i64, i64)>]) -> Vec<(usize, i64)> {
    let n = tasks.len();
    let mut updates = Vec::new();

    for i in 0..n {
        let Some((est_i, _lct_i, dur_i)) = tasks[i] else {
            continue;
        };
        let mut best_new_est = est_i;

        for (b_idx, task_b) in tasks.iter().enumerate() {
            if b_idx == i {
                continue;
            }
            let Some((_, b, _)) = *task_b else { continue };

            let mut p_omega = 0i64;
            let mut est_omega = i64::MAX;
            let mut any = false;
            for (j, task_j) in tasks.iter().enumerate() {
                if j == i {
                    continue;
                }
                let Some((est_j, lct_j, dur_j)) = *task_j else {
                    continue;
                };
                if lct_j > b {
                    continue;
                }
                any = true;
                p_omega = p_omega.saturating_add(dur_j);
                est_omega = est_omega.min(est_j);
            }
            if !any || p_omega <= 0 {
                continue;
            }
            // Omega must be individually feasible for the theorem's precondition to hold; if it
            // isn't, some window check elsewhere already reports Conflict — safe to just skip.
            if p_omega > b.saturating_sub(est_omega) {
                continue;
            }

            let est_with_i = est_omega.min(est_i);
            if est_with_i.saturating_add(p_omega).saturating_add(dur_i) > b {
                let candidate = est_omega.saturating_add(p_omega);
                if candidate > best_new_est {
                    best_new_est = candidate;
                }
            }
        }

        if best_new_est > est_i {
            updates.push((i, best_new_est));
        }
    }

    updates
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

        // Edge-finding bound update (see `edge_finding_bound_updates`'s doc comment for the
        // theorem and soundness proof pointer): tightens a task's earliest start using the
        // combined duration of *sets* of other tasks, catching pushes the pairwise precedence
        // loop below (which only ever reasons about one other task at a time) cannot.
        let task_bounds: Vec<Option<(i64, i64, i64)>> = self
            .tasks
            .iter()
            .map(|task| {
                let (min, max) = domain_bounds(domains, task.start)?;
                let lct = max.saturating_add(duration_as_i64(task.duration));
                Some((min, lct, duration_as_i64(task.duration)))
            })
            .collect();
        for (idx, new_est) in edge_finding_bound_updates(&task_bounds) {
            let var = self.tasks[idx].start;
            if let Some(result) = prune(domains, &mut changed, var, |d| d.remove_below(new_est)) {
                return result;
            }
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

    /// Edge-finding bound update, hand-verified case: two tasks `a`,`b` (domain `[0,3]`,
    /// duration 2 each — together they need 4 time units within `[0,5)`, individually feasible)
    /// and a third task `c` (domain `[0,10]`, duration 2). Neither `a` nor `b` alone pushes `c`
    /// via pairwise precedence (each only needs `c` to end after its own *latest* start, which
    /// `c` finishing at 2 never does against a max of 3) — only the *combined* `Omega = {a, b}`
    /// forces `c` to start at or after `est(Omega) + p(Omega) = 0 + 4 = 4`.
    ///
    /// Manually verified this bound is exactly tight: `c` at `s=3` always collides with `a`/`b`
    /// regardless of how they're placed within `[0,5)` (pigeonhole: `a`,`b` need 4 of the 5 units
    /// in `[0,5)`, so at least 1 unit of their footprint falls in `[3,5)`, wherever `c` would
    /// sit), but `c` at `s=4` is achievable (`a@[0,2)`, `b@[2,4)`, `c@[4,6)` — all disjoint).
    #[test]
    fn test_propagate_edge_finding_tightens_est_beyond_pairwise_precedence() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        domains.insert(a, Domain::range(0, 3));
        domains.insert(b, Domain::range(0, 3));
        domains.insert(c, Domain::range(0, 10));
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

        let result = constraint.propagate(&mut trailed);

        assert_eq!(result, PropagationResult::Success { changed: true });
        assert_eq!(
            trailed.get(&a).unwrap().values(),
            (0..=3).collect::<Vec<_>>()
        );
        assert_eq!(
            trailed.get(&b).unwrap().values(),
            (0..=3).collect::<Vec<_>>()
        );
        assert_eq!(
            trailed.get(&c).unwrap().min(),
            Some(4),
            "c must start at or after a and b (combined) finish, even though neither alone forces it"
        );
    }

    /// Sanity check that the update doesn't over-tighten: widening `c`'s domain enough that it no
    /// longer needs to be pushed (there's room for it before `a`/`b` even without edge-finding
    /// reasoning) must leave `c` untouched by this specific mechanism.
    #[test]
    fn test_propagate_edge_finding_no_update_when_not_forced() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        domains.insert(a, Domain::range(5, 8));
        domains.insert(b, Domain::range(5, 8));
        domains.insert(c, Domain::range(0, 3));
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

        constraint.propagate(&mut trailed);

        assert_eq!(
            trailed.get(&c).unwrap().min(),
            Some(0),
            "c already fits entirely before a/b's earliest possible start; no push needed"
        );
    }
}
