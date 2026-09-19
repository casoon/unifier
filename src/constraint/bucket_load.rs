//! Cumulative load cap per discrete value bucket.
//!
//! Generalizes "the total occupied duration an entity accumulates inside each recurring period
//! bucket must stay within `limit`" — the primitive behind a maximum-daily-load style rule —
//! without any scheduling vocabulary. A *task* is a plain decision variable over integer time
//! together with an occupied length and a per-time-unit demand; a *bucket* is an explicit
//! half-open value range. The mapping from a real calendar (days, weeks, ...) onto bucketed value
//! ranges is the caller's job, so this module stays domain-agnostic.
//!
//! Unlike [`crate::constraint::Cumulative`] — which bounds the *simultaneous* load at any instant
//! — this constraint bounds the *summed* load accumulated across a whole bucket, e.g. all lessons
//! scheduled within one day.
//!
//! Reference:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.
//!   (Energetic reasoning: cumulative resource usage over a time window.)

use crate::constraint::{Assignment, Constraint, Explanation, PropagationResult};
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use std::collections::HashMap;

/// One assignment that contributes `demand * overlap` occupied time to every bucket its interval
/// `[start, start + duration)` overlaps with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketedTask {
    /// Variable holding the task's start value.
    pub start: VariableId,
    /// Occupied length in time units (`>= 0`).
    pub duration: i64,
    /// Demand per time unit. An overlap of `L` time units contributes `demand * L`.
    pub demand: i64,
    /// Optional presence variable (`1` = present). `None` means unconditionally present. A task
    /// whose presence is assigned `0` contributes nothing.
    pub presence: Option<VariableId>,
}

impl BucketedTask {
    /// Creates an unconditionally present task.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(start: VariableId, duration: i64, demand: i64) -> Self {
        Self {
            start,
            duration,
            demand,
            presence: None,
        }
    }

    /// Attaches a presence variable; the task only counts while `presence` is `1`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn with_presence(mut self, presence: VariableId) -> Self {
        self.presence = Some(presence);
        self
    }
}

/// A half-open value range `[start, end)` mapped to bucket `bucket`.
///
/// Ranges of one constraint should be non-overlapping; the same `bucket` may be used by several
/// (e.g. disjoint) ranges, or by ranges repeated across a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketRange {
    /// Inclusive lower bound.
    pub start: i64,
    /// Exclusive upper bound.
    pub end: i64,
    /// Bucket this range belongs to.
    pub bucket: usize,
}

impl BucketRange {
    /// Creates a bucket range.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub const fn new(start: i64, end: i64, bucket: usize) -> Self {
        Self { start, end, bucket }
    }
}

/// Caps the summed `demand * occupied time` of all [`BucketedTask`]s inside every [`BucketRange`]
/// at `limit`.
///
/// A task's contribution is attributed to the bucket(s) its interval `[start, start + duration)`
/// overlaps, split proportionally at range boundaries, so a task spanning two buckets contributes
/// its overlap to each. A value outside every range contributes to no bucket.
#[derive(Debug, Clone)]
pub struct MaximumBucketLoad {
    tasks: Vec<BucketedTask>,
    ranges: Vec<BucketRange>,
    limit: i64,
    bucket_count: usize,
    scope: Vec<VariableId>,
}

impl MaximumBucketLoad {
    /// Creates a load cap, sorting `ranges` by start so overlap lookup is a binary search.
    ///
    /// # Complexity
    /// Time: O(T + R log R) where T = `tasks.len()`, R = `ranges.len()`.
    /// Space: O(T + R + B) where B = number of distinct buckets.
    pub fn new(
        tasks: impl IntoIterator<Item = BucketedTask>,
        ranges: impl IntoIterator<Item = BucketRange>,
        limit: i64,
    ) -> Self {
        let tasks: Vec<BucketedTask> = tasks.into_iter().collect();
        let mut ranges: Vec<BucketRange> = ranges.into_iter().collect();
        ranges.sort_unstable_by_key(|range| range.start);
        let bucket_count = ranges
            .iter()
            .map(|range| range.bucket + 1)
            .max()
            .unwrap_or(0);
        let mut scope = Vec::new();
        for task in &tasks {
            scope.push(task.start);
            if let Some(presence) = task.presence {
                scope.push(presence);
            }
        }
        scope.sort_unstable();
        scope.dedup();
        Self {
            tasks,
            ranges,
            limit,
            bucket_count,
            scope,
        }
    }

    /// Returns the configured limit.
    ///
    /// Time complexity: O(1).
    pub const fn limit(&self) -> i64 {
        self.limit
    }

    /// Accumulates the load per bucket for the currently determined tasks.
    ///
    /// A task counts only if it is present (`presence` unset or assigned `1`) *and* its start value
    /// is assigned; anything else contributes nothing, keeping a partial assignment from being
    /// flagged before all its tasks are known.
    fn bucket_loads(&self, assignment: &HashMap<VariableId, i64>) -> Vec<i64> {
        let mut loads = vec![0i64; self.bucket_count];
        if self.bucket_count == 0 {
            return loads;
        }
        for task in &self.tasks {
            if !task_is_present(assignment, task.presence) {
                continue;
            }
            let Some(&start) = assignment.get(&task.start) else {
                continue;
            };
            let end = start.saturating_add(task.duration);
            for (bucket, overlap) in self.overlaps(start, end) {
                let contribution = task.demand.saturating_mul(overlap);
                loads[bucket] = loads[bucket].saturating_add(contribution);
            }
        }
        loads
    }

    /// Returns `(bucket, overlap)` for every bucket range overlapping `[start, end)`.
    fn overlaps(&self, start: i64, end: i64) -> Vec<(usize, i64)> {
        if self.ranges.is_empty() || end <= start {
            return Vec::new();
        }
        // First range that could overlap: the last one starting at or before `start` (ranges are
        // sorted by start). Everything before it ends no later than that range starts, so it
        // cannot overlap.
        let first = self
            .ranges
            .partition_point(|range| range.start <= start)
            .saturating_sub(1);
        let mut overlaps = Vec::new();
        for range in &self.ranges[first..] {
            if range.start >= end {
                break;
            }
            let overlap_start = start.max(range.start);
            let overlap_end = end.min(range.end);
            if overlap_end > overlap_start {
                overlaps.push((range.bucket, overlap_end - overlap_start));
            }
        }
        overlaps
    }

    fn first_excess(&self, assignment: &HashMap<VariableId, i64>) -> Option<(usize, i64)> {
        self.bucket_loads(assignment)
            .into_iter()
            .enumerate()
            .find(|&(_, load)| load > self.limit)
    }
}

/// A task contributes only when its presence variable is unset (mandatory) or explicitly `1`.
fn task_is_present(assignment: &HashMap<VariableId, i64>, presence: Option<VariableId>) -> bool {
    match presence {
        None => true,
        Some(presence) => assignment.get(&presence) == Some(&1),
    }
}

impl Constraint for MaximumBucketLoad {
    fn name(&self) -> &str {
        "MaximumBucketLoad"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    /// The load sum is monotone: adding tasks or occupied time can only increase it, so an
    /// already-exceeded bucket from a *partial* assignment can never be repaired by completing the
    /// remaining variables.
    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        self.first_excess(assignment).is_none()
    }

    /// The total excess across buckets, not the number of buckets over their limit: a move that
    /// takes one hour out of a bucket still three hours over has changed something, and the
    /// search needs to see it.
    fn violations(&self, assignment: &HashMap<VariableId, i64>) -> u32 {
        self.bucket_loads(assignment)
            .into_iter()
            .filter_map(|load| u32::try_from(load.saturating_sub(self.limit)).ok())
            .fold(0u32, u32::saturating_add)
    }

    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        let (bucket, load) = self.first_excess(assignment)?;
        Some(Explanation {
            constraint_name: "MaximumBucketLoad",
            involved: self.scope.clone(),
            message: format!(
                "bucket {bucket} load {load} exceeds the limit {}",
                self.limit
            ),
        })
    }

    /// No domain filtering: a bucket only exceeds its limit once enough tasks are placed, which a
    /// partial domain view cannot decide soundly without reasoning over every task's remaining
    /// positions. The hard cap is still enforced by [`Self::is_satisfied`] during search.
    fn propagate(&self, _domains: &mut TrailedDomains) -> PropagationResult {
        PropagationResult::Success { changed: false }
    }

    fn validate(&self) -> Result<(), String> {
        if self.limit < 0 {
            return Err("load limit must not be negative".to_string());
        }
        for task in &self.tasks {
            if task.duration < 0 {
                return Err("task duration must not be negative".to_string());
            }
            if task.demand < 0 {
                return Err("task demand must not be negative".to_string());
            }
        }
        let mut previous_end: Option<i64> = None;
        for range in &self.ranges {
            if range.start >= range.end {
                return Err("bucket range must be non-empty (start < end)".to_string());
            }
            if let Some(end) = previous_end
                && range.start < end
            {
                return Err("bucket ranges must not overlap".to_string());
            }
            previous_end = Some(range.end);
        }
        Ok(())
    }
}

/// Enforces that the consecutive blocks a group of tasks occupies form one of the allowed
/// multisets of block durations.
///
/// Where [`MaximumBucketLoad`] *sums* occupied time inside a bucket, this constraint judges the
/// **shape** the occupied intervals form: within every [`BucketRange`] the intervals are clipped,
/// touching or overlapping intervals merge into one block, and the resulting block durations of
/// all ranges together are compared — order irrelevant — against `allowed`. `[2, 1, 1]` therefore
/// means "one block of two plus two blocks of one", no matter which bucket each block lands in.
///
/// Only the shape is judged, so a task's [`BucketedTask::demand`] is ignored here; the buckets
/// themselves are the caller's mapping from a calendar onto value ranges, keeping this module free
/// of scheduling vocabulary. The aggregation over *all* ranges mirrors
/// `schedulr::bucket_load_pattern`, so the solver and a caller reasoning about teaching blocks
/// share one notion of a block.
///
/// A partial assignment is never a violation: the block multiset is not monotone — a later task
/// can split or join a block — so an undecided task keeps the check optimistic. The constraint is
/// enforced as a hard check once the search has placed every task.
///
/// Reference:
/// - van Hoeve, W. J., & Katriel, I. (2006). *Global Constraints*. Handbook of Constraint
///   Programming, Chapter 6. (Global constraints over sets of intervals.)
#[derive(Debug, Clone)]
pub struct BucketBlockPattern {
    tasks: Vec<BucketedTask>,
    ranges: Vec<BucketRange>,
    allowed: Vec<Vec<i64>>,
    scope: Vec<VariableId>,
}

impl BucketBlockPattern {
    /// Creates the constraint, normalizing every allowed pattern to a descending block multiset so
    /// the comparison is order-free.
    ///
    /// # Complexity
    /// Time: O(T + R log R + A · P log P) where T = `tasks.len()`, R = `ranges.len()`,
    /// A = number of allowed patterns, P = length of the longest pattern.
    /// Space: O(T + R + A · P).
    pub fn new(
        tasks: impl IntoIterator<Item = BucketedTask>,
        ranges: impl IntoIterator<Item = BucketRange>,
        allowed: impl IntoIterator<Item = Vec<i64>>,
    ) -> Self {
        let tasks: Vec<BucketedTask> = tasks.into_iter().collect();
        let mut ranges: Vec<BucketRange> = ranges.into_iter().collect();
        ranges.sort_unstable_by_key(|range| range.start);
        let allowed: Vec<Vec<i64>> = allowed
            .into_iter()
            .map(|mut pattern| {
                pattern.sort_unstable_by(|left, right| right.cmp(left));
                pattern
            })
            .collect();
        let mut scope = Vec::new();
        for task in &tasks {
            scope.push(task.start);
            if let Some(presence) = task.presence {
                scope.push(presence);
            }
        }
        scope.sort_unstable();
        scope.dedup();
        Self {
            tasks,
            ranges,
            allowed,
            scope,
        }
    }

    /// Returns the normalized allowed block multisets, each descending.
    ///
    /// Time complexity: O(1).
    pub fn allowed(&self) -> &[Vec<i64>] {
        &self.allowed
    }

    /// Whether every task's start — and, if it has one, its presence — is decided. The block shape
    /// can only be judged once no further task can still change it.
    fn fully_determined(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        self.tasks.iter().all(|task| {
            assignment.contains_key(&task.start)
                && task
                    .presence
                    .is_none_or(|presence| assignment.contains_key(&presence))
        })
    }

    /// The occupied blocks of all decided tasks, aggregated over every range, descending.
    ///
    /// Mirrors `schedulr::bucket_load_blocks`/`bucket_load_pattern`: blocks are consecutive runs,
    /// so two touching single slots count as one block of two.
    fn observed_pattern(&self, assignment: &HashMap<VariableId, i64>) -> Vec<i64> {
        let mut durations: Vec<i64> = Vec::new();
        for range in &self.ranges {
            let mut segments: Vec<(i64, i64)> = self
                .tasks
                .iter()
                .filter(|task| task_is_present(assignment, task.presence))
                .filter_map(|task| {
                    let &start = assignment.get(&task.start)?;
                    let end = start.saturating_add(task.duration);
                    let overlap_start = start.max(range.start);
                    let overlap_end = end.min(range.end);
                    (overlap_end > overlap_start).then_some((overlap_start, overlap_end))
                })
                .collect();
            segments.sort_unstable();

            let mut blocks: Vec<(i64, i64)> = Vec::new();
            for (start, end) in segments {
                match blocks.last_mut() {
                    // Touching counts as one block: a double period occupies adjacent slots.
                    Some(block) if start <= block.1 => block.1 = block.1.max(end),
                    _ => blocks.push((start, end)),
                }
            }
            durations.extend(blocks.into_iter().map(|(start, end)| end - start));
        }
        durations.sort_unstable_by(|left, right| right.cmp(left));
        durations
    }

    /// Whether `observed` equals one of the allowed multisets (both descending).
    fn matches(&self, observed: &[i64]) -> bool {
        self.allowed.iter().any(|allowed| allowed == observed)
    }
}

/// Renders a block multiset for an explanation, e.g. `[2, 1, 1]`.
fn render_pattern(pattern: &[i64]) -> String {
    let blocks: Vec<String> = pattern.iter().map(i64::to_string).collect();
    format!("[{}]", blocks.join(", "))
}

impl Constraint for BucketBlockPattern {
    fn name(&self) -> &str {
        "BucketBlockPattern"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    /// Deliberately optimistic on a partial assignment: until every task is placed the block
    /// multiset is still open, and rejecting early would prune sound solutions.
    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        if !self.fully_determined(assignment) {
            return true;
        }
        self.matches(&self.observed_pattern(assignment))
    }

    /// The distance to the nearest allowed shape: how many hours would have to move between
    /// blocks to reach it, block by block against the closest candidate. A shape one hour off
    /// therefore counts less than one that is three off, which is the gradient a repair follows.
    ///
    /// Optimistic on a partial assignment for the same reason [`Self::is_satisfied`] is: until
    /// every task is placed the shape is still open.
    fn violations(&self, assignment: &HashMap<VariableId, i64>) -> u32 {
        if !self.fully_determined(assignment) {
            return 0;
        }
        let observed = self.observed_pattern(assignment);
        self.allowed
            .iter()
            .map(|allowed| {
                let width = allowed.len().max(observed.len());
                (0..width)
                    .map(|index| {
                        let wanted = allowed.get(index).copied().unwrap_or(0);
                        let got = observed.get(index).copied().unwrap_or(0);
                        u32::try_from(wanted.abs_diff(got)).unwrap_or(u32::MAX)
                    })
                    .fold(0u32, u32::saturating_add)
            })
            .min()
            // No allowed shape at all: nothing can match, so the count must not say "fine".
            .unwrap_or(1)
    }

    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        if !self.fully_determined(assignment) {
            return None;
        }
        let observed = self.observed_pattern(assignment);
        if self.matches(&observed) {
            return None;
        }
        let allowed: Vec<String> = self
            .allowed
            .iter()
            .map(|pattern| render_pattern(pattern))
            .collect();
        Some(Explanation {
            constraint_name: "BucketBlockPattern",
            involved: self.scope.clone(),
            message: format!(
                "blocks {} do not match any allowed pattern {}",
                render_pattern(&observed),
                allowed.join(" or ")
            ),
        })
    }

    /// No domain filtering: which block shapes remain reachable depends on the joint placement of
    /// every task, which a per-variable domain view cannot decide soundly. The hard check is still
    /// enforced by [`Self::is_satisfied`] during search.
    fn propagate(&self, _domains: &mut TrailedDomains) -> PropagationResult {
        PropagationResult::Success { changed: false }
    }

    fn validate(&self) -> Result<(), String> {
        if self.allowed.is_empty() {
            return Err("at least one allowed block pattern is required".to_string());
        }
        for pattern in &self.allowed {
            if pattern.is_empty() {
                return Err("an allowed block pattern must not be empty".to_string());
            }
            if pattern.iter().any(|block| *block <= 0) {
                return Err("block durations must be positive".to_string());
            }
        }
        for task in &self.tasks {
            if task.duration < 0 {
                return Err("task duration must not be negative".to_string());
            }
        }
        let mut previous_end: Option<i64> = None;
        for range in &self.ranges {
            if range.start >= range.end {
                return Err("bucket range must be non-empty (start < end)".to_string());
            }
            if let Some(end) = previous_end
                && range.start < end
            {
                return Err("bucket ranges must not overlap".to_string());
            }
            previous_end = Some(range.end);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;

    fn assignment(entries: &[(VariableId, i64)]) -> HashMap<VariableId, i64> {
        entries.iter().copied().collect()
    }

    /// Two unit-length tasks that both land in day 0, with a limit of exactly 2, must be allowed.
    #[test]
    fn exactly_at_the_limit_is_allowed() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MaximumBucketLoad::new(
            [BucketedTask::new(a, 1, 1), BucketedTask::new(b, 1, 1)],
            [BucketRange::new(0, 5, 0)],
            2,
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 0), (b, 3)])));
    }

    /// One more unit than the limit in the same bucket must be rejected.
    #[test]
    fn above_the_limit_is_rejected() {
        let (a, b, c) = (VariableId(0), VariableId(1), VariableId(2));
        let constraint = MaximumBucketLoad::new(
            [
                BucketedTask::new(a, 1, 1),
                BucketedTask::new(b, 1, 1),
                BucketedTask::new(c, 1, 1),
            ],
            [BucketRange::new(0, 5, 0)],
            2,
        );
        assert!(!constraint.is_satisfied(&assignment(&[(a, 0), (b, 3), (c, 4)])));
        let explanation = constraint
            .explain(&assignment(&[(a, 0), (b, 3), (c, 4)]))
            .expect("violation is explained");
        assert_eq!(explanation.constraint_name, "MaximumBucketLoad");
        assert!(explanation.message.contains("limit 2"));
    }

    /// A task whose interval crosses a bucket boundary contributes its overlap to each bucket.
    #[test]
    fn occupied_time_is_split_at_bucket_boundaries() {
        let a = VariableId(0);
        // [3, 6) overlaps day 0 ([0, 5)) by 2 and day 1 ([5, 10)) by 1.
        let constraint = MaximumBucketLoad::new(
            [BucketedTask::new(a, 3, 1)],
            [BucketRange::new(0, 5, 0), BucketRange::new(5, 10, 1)],
            2,
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 3)])));

        let stricter = MaximumBucketLoad::new(
            [BucketedTask::new(a, 3, 1)],
            [BucketRange::new(0, 5, 0), BucketRange::new(5, 10, 1)],
            1,
        );
        assert!(
            !stricter.is_satisfied(&assignment(&[(a, 3)])),
            "day 0 receives 2 units, which exceeds the limit of 1"
        );
    }

    /// A value-range boundary belongs to the range that starts there (half-open), so a task placed
    /// exactly at a bucket boundary counts entirely in the new bucket.
    #[test]
    fn bucket_boundaries_are_half_open() {
        let a = VariableId(0);
        let ranges = [BucketRange::new(0, 5, 0), BucketRange::new(5, 10, 1)];
        let at_boundary = MaximumBucketLoad::new([BucketedTask::new(a, 1, 1)], ranges, 1);
        assert!(
            at_boundary.is_satisfied(&assignment(&[(a, 5)])),
            "start 5 belongs to bucket 1, not bucket 0"
        );

        let just_below = MaximumBucketLoad::new([BucketedTask::new(a, 1, 1)], ranges, 0);
        assert!(
            !just_below.is_satisfied(&assignment(&[(a, 4)])),
            "start 4 is the last value of bucket 0"
        );
    }

    /// A presence variable assigned `0` removes the task's contribution entirely; unset presence
    /// stays optimistic.
    #[test]
    fn absent_tasks_do_not_count() {
        let (a, presence) = (VariableId(0), VariableId(9));
        let constraint = MaximumBucketLoad::new(
            [BucketedTask::new(a, 4, 1).with_presence(presence)],
            [BucketRange::new(0, 5, 0)],
            1,
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 0), (presence, 0)])));
        assert!(!constraint.is_satisfied(&assignment(&[(a, 0), (presence, 1)])));
        assert!(
            constraint.is_satisfied(&assignment(&[(a, 0)])),
            "an undecided presence must not yet be counted as load"
        );
    }

    /// Multiple participants/resources per activity: two tasks with the same start but different
    /// demand each add their own contribution (the caller builds one constraint per entity).
    #[test]
    fn demand_scales_the_contribution() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = MaximumBucketLoad::new(
            [BucketedTask::new(a, 2, 2), BucketedTask::new(b, 1, 1)],
            [BucketRange::new(0, 10, 0)],
            4,
        );
        // a contributes 2 * 2 = 4, b contributes 1: total 5 > 4.
        assert!(!constraint.is_satisfied(&assignment(&[(a, 0), (b, 0)])));
        let restricted = MaximumBucketLoad::new(
            [BucketedTask::new(a, 2, 2), BucketedTask::new(b, 1, 1)],
            [BucketRange::new(0, 10, 0)],
            5,
        );
        assert!(restricted.is_satisfied(&assignment(&[(a, 0), (b, 0)])));
    }

    /// A value outside every range is not attributed to any bucket.
    #[test]
    fn values_outside_all_buckets_are_ignored() {
        let a = VariableId(0);
        let constraint =
            MaximumBucketLoad::new([BucketedTask::new(a, 1, 1)], [BucketRange::new(0, 5, 0)], 0);
        assert!(constraint.is_satisfied(&assignment(&[(a, 7)])));
    }

    /// End-to-end through the DSL: a cap of 1 unit per bucket forces the solver to spread two
    /// otherwise-identical activities across two buckets.
    #[test]
    fn builder_and_solver_respect_the_bucket_cap() {
        use crate::constraint::bucket_load::{BucketRange as Range, BucketedTask as Task};
        use crate::dsl::ModelBuilder;
        use crate::solver::{BacktrackingSolver, SolverOptions};

        let mut builder = ModelBuilder::new();
        let a = builder.new_var("a", 0..=1);
        let b = builder.new_var("b", 0..=1);
        builder.add_maximum_bucket_load(
            [Task::new(a, 1, 1), Task::new(b, 1, 1)],
            [Range::new(0, 1, 0), Range::new(1, 2, 1)],
            1,
        );
        let graph = builder.build().expect("model should validate");
        let solution = BacktrackingSolver::new()
            .solve(&graph, &SolverOptions::default())
            .solution
            .expect("feasible: the two tasks can occupy different buckets");
        assert_ne!(solution.assignment[&a], solution.assignment[&b]);
    }

    /// Standalone propagation: the constraint deliberately prunes nothing, staying sound.
    #[test]
    fn propagate_is_a_no_op() {
        let a = VariableId(0);
        let constraint =
            MaximumBucketLoad::new([BucketedTask::new(a, 1, 1)], [BucketRange::new(0, 5, 0)], 1);
        let mut domains = TrailedDomains::new(HashMap::from([(a, Domain::range(0, 5))]));
        assert_eq!(
            constraint.propagate(&mut domains),
            PropagationResult::Success { changed: false }
        );
    }

    /// Two unit slots placed next to each other are **one** block of two, which is what a double
    /// period means — a duration multiset that ignored adjacency would call this "[1, 1]".
    #[test]
    fn touching_slots_are_one_block() {
        let (a, b) = (VariableId(0), VariableId(1));
        let tasks = || [BucketedTask::new(a, 1, 1), BucketedTask::new(b, 1, 1)];
        let ranges = [BucketRange::new(0, 5, 0)];
        let adjacent = assignment(&[(a, 1), (b, 2)]);

        let double = BucketBlockPattern::new(tasks(), ranges, [vec![2]]);
        assert!(
            double.is_satisfied(&adjacent),
            "slots 1 and 2 touch and therefore form a block of two"
        );

        let singles = BucketBlockPattern::new(tasks(), ranges, [vec![1, 1]]);
        assert!(
            !singles.is_satisfied(&adjacent),
            "adjacent slots must not be read as two separate single blocks"
        );
    }

    /// The same two slots one apart are two single blocks — and under a `[2]`-only pattern that is
    /// a violation, so the constraint discriminates adjacency in both directions.
    #[test]
    fn a_gap_splits_the_block() {
        let (a, b) = (VariableId(0), VariableId(1));
        let tasks = || [BucketedTask::new(a, 1, 1), BucketedTask::new(b, 1, 1)];
        let ranges = [BucketRange::new(0, 5, 0)];
        let apart = assignment(&[(a, 1), (b, 3)]);

        assert!(BucketBlockPattern::new(tasks(), ranges, [vec![1, 1]]).is_satisfied(&apart));
        assert!(!BucketBlockPattern::new(tasks(), ranges, [vec![2]]).is_satisfied(&apart));
    }

    /// Blocks may sit in different buckets: the pattern aggregates over all ranges, so a double
    /// period on one day and a single on another matches `[2, 1]`.
    #[test]
    fn blocks_are_aggregated_across_buckets() {
        let (a, b) = (VariableId(0), VariableId(1));
        let ranges = [BucketRange::new(0, 2, 0), BucketRange::new(5, 7, 1)];
        let constraint = BucketBlockPattern::new(
            [BucketedTask::new(a, 2, 1), BucketedTask::new(b, 1, 1)],
            ranges,
            [vec![2, 1]],
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 0), (b, 5)])));
        assert!(
            !constraint.is_satisfied(&assignment(&[(a, 0), (b, 3)])),
            "b at 3 lies outside every bucket, so only the double period remains"
        );
    }

    /// A task whose slot lies outside every bucket contributes no block, mirroring
    /// [`MaximumBucketLoad`].
    #[test]
    fn values_outside_all_buckets_contribute_no_block() {
        let a = VariableId(0);
        let constraint = BucketBlockPattern::new(
            [BucketedTask::new(a, 1, 1)],
            [BucketRange::new(0, 5, 0)],
            [vec![1]],
        );
        assert!(!constraint.is_satisfied(&assignment(&[(a, 7)])));
    }

    /// Until every task is placed the block multiset is still open, so the check stays optimistic.
    #[test]
    fn a_partial_assignment_is_never_a_violation() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = BucketBlockPattern::new(
            [BucketedTask::new(a, 1, 1), BucketedTask::new(b, 1, 1)],
            [BucketRange::new(0, 5, 0)],
            [vec![2]],
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 1)])));
        assert!(
            !constraint.is_satisfied(&assignment(&[(a, 1), (b, 3)])),
            "once both slots are placed the gap makes the block shape [1, 1]"
        );
    }

    /// An undecided presence also keeps the check open, because the task may still add a block.
    #[test]
    fn an_undecided_presence_is_not_judged() {
        let (a, presence) = (VariableId(0), VariableId(9));
        let constraint = BucketBlockPattern::new(
            [BucketedTask::new(a, 2, 1).with_presence(presence)],
            [BucketRange::new(0, 5, 0)],
            [vec![2]],
        );
        assert!(constraint.is_satisfied(&assignment(&[(a, 0)])));
        assert!(constraint.is_satisfied(&assignment(&[(a, 0), (presence, 1)])));
        assert!(
            !constraint.is_satisfied(&assignment(&[(a, 0), (presence, 0)])),
            "an absent task contributes no block, so [] matches nothing here"
        );
    }

    /// The violation names both the observed shape and the allowed ones.
    #[test]
    fn the_explanation_names_the_mismatch() {
        let (a, b) = (VariableId(0), VariableId(1));
        let constraint = BucketBlockPattern::new(
            [BucketedTask::new(a, 1, 1), BucketedTask::new(b, 1, 1)],
            [BucketRange::new(0, 5, 0)],
            [vec![2]],
        );
        let assignment = assignment(&[(a, 0), (b, 2)]);
        let explanation = constraint
            .explain(&assignment)
            .expect("a mismatch is explained");
        assert_eq!(explanation.constraint_name, "BucketBlockPattern");
        assert!(
            explanation.message.contains("[1, 1]"),
            "{}",
            explanation.message
        );
        assert!(
            explanation.message.contains("[2]"),
            "{}",
            explanation.message
        );
    }

    /// End-to-end through the DSL and the solver: a course that allows only double periods must end
    /// up with its two slots adjacent, even though nothing else forces that.
    #[test]
    fn builder_and_solver_respect_the_block_pattern() {
        use crate::constraint::bucket_load::{BucketRange as Range, BucketedTask as Task};
        use crate::dsl::ModelBuilder;
        use crate::solver::{BacktrackingSolver, SolverOptions};

        let mut builder = ModelBuilder::new();
        let a = builder.new_var("a", 0..=3);
        let b = builder.new_var("b", 0..=3);
        builder.add_bucket_block_pattern(
            [Task::new(a, 1, 1), Task::new(b, 1, 1)],
            [Range::new(0, 4, 0)],
            [vec![2]],
        );
        let graph = builder.build().expect("model should validate");
        let solution = BacktrackingSolver::new()
            .solve(&graph, &SolverOptions::default())
            .solution
            .expect("feasible: the two slots can touch");
        assert_eq!(
            (solution.assignment[&a] - solution.assignment[&b]).abs(),
            1,
            "only adjacent placements form the required block of two"
        );
    }
}
