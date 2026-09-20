//! High-level DSL surface API for building CSP/COP models.
//!
//! Provides fluent builders for creating variables, domains, intervals, resources,
//! activities, and constraint specifications.

use crate::constraint::no_overlap::TaskInterval;
use crate::constraint::{
    AllDifferent, AllowedValues, AtLeast, AtMost, BucketBlockPattern, BucketRange, BucketedTask,
    Constraint, Cumulative, Equal, ExactlyOne, ForbiddenValues, LessThanOrEqual, MaximumBucketLoad,
    MinimumDistance, NoOverlap, NotEqual, Optional, PeriodicValues, Precedence, TaskDemand,
};
use crate::model::activity::{Activity, ActivityId};
use crate::model::domain::Domain;
use crate::model::group::{Group, GroupId};
use crate::model::interval::{DurationSpec, Interval};
use crate::model::resource::{Resource, ResourceId};
use crate::model::variable::{Variable, VariableId};
use crate::propagation::graph::{ConstraintGraph, ConstraintId, ModelError, ValidatedGraph};
use crate::score::{CategorizedObjective, Objective, ScoreLevel, WeightedSum};
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

/// Fluent builder for constructing CSP/COP models and constraint hypergraphs.
#[derive(Debug, Default)]
pub struct ModelBuilder {
    graph: ConstraintGraph,
    next_var_id: u32,
    next_resource_id: u32,
    next_activity_id: u32,
    next_group_id: u32,
}

impl ModelBuilder {
    /// Creates a new empty model builder.
    ///
    /// Time & Space: O(1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a decision variable with a range domain `[min..=max]`.
    ///
    /// # Complexity
    /// Time & Space: O(1) amortized.
    pub fn new_var(&mut self, name: impl Into<String>, range: RangeInclusive<i64>) -> VariableId {
        let id = VariableId(self.next_var_id);
        self.next_var_id += 1;
        let var = Variable::new(id, name);
        let domain = Domain::range(*range.start(), *range.end());
        self.graph.add_variable(var, domain);
        id
    }

    /// Adds a presence variable (domain `{0, 1}`) for use with [`Self::add_optional`] — `1` means
    /// present/active, `0` means absent.
    ///
    /// # Complexity
    /// Time & Space: O(1) amortized.
    pub fn new_presence_var(&mut self, name: impl Into<String>) -> VariableId {
        self.new_var(name, 0..=1)
    }

    /// Adds an interval variable tuple `[start, start + duration)`.
    pub fn new_interval(
        &mut self,
        name_prefix: &str,
        start_range: RangeInclusive<i64>,
        duration: u64,
        end_range: RangeInclusive<i64>,
    ) -> Interval {
        let start = self.new_var(format!("{}_start", name_prefix), start_range);
        let end = self.new_var(format!("{}_end", name_prefix), end_range);
        let interval = Interval::new(start, DurationSpec::Fixed(duration), end);

        // Enforce implicit end = start + duration constraint
        self.graph.add_constraint(Arc::new(Equal::new(
            end,
            start,
            crate::constraint::duration_as_i64(duration),
        )));

        interval
    }

    /// Adds a resource with given capacity.
    pub fn new_resource(&mut self, name: impl Into<String>, capacity: u32) -> Resource {
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;
        Resource::new(id, name, capacity)
    }

    /// Adds an activity bound to an interval.
    pub fn new_activity(&mut self, name: impl Into<String>, interval: Interval) -> Activity {
        let id = ActivityId(self.next_activity_id);
        self.next_activity_id += 1;
        Activity::new(id, name, interval)
    }

    /// Adds a group sharing a common interval.
    pub fn new_group(&mut self, name: impl Into<String>, shared_interval: Interval) -> Group {
        let id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        Group::new(id, name, shared_interval)
    }

    /// Adds a custom constraint implementation to the model.
    pub fn add_constraint(&mut self, constraint: Arc<dyn Constraint>) -> ConstraintId {
        self.graph.add_constraint(constraint)
    }

    /// Adds `constraint` as optional: it only applies while `presence` is (or can still become)
    /// `1` — see [`Optional`] for the exact propagation semantics and [`Self::new_presence_var`]
    /// to create `presence`.
    ///
    /// Returns the id of the wrapping constraint, so a caller that keeps a map from constraint
    /// to the entity it speaks for can name this one too — a violation reported as
    /// "ForbiddenValues is violated" tells nobody whose calendar was hit.
    pub fn add_optional(
        &mut self,
        constraint: Arc<dyn Constraint>,
        presence: VariableId,
    ) -> ConstraintId {
        self.graph
            .add_constraint(Arc::new(Optional::new(constraint, presence)))
    }

    /// Adds an `Equal` constraint `v1 = v2 + offset`.
    pub fn add_equal(&mut self, v1: VariableId, v2: VariableId, offset: i64) {
        self.graph
            .add_constraint(Arc::new(Equal::new(v1, v2, offset)));
    }

    /// Adds a `NotEqual` constraint `v1 != v2`.
    pub fn add_not_equal(&mut self, v1: VariableId, v2: VariableId) {
        self.graph.add_constraint(Arc::new(NotEqual::new(v1, v2)));
    }

    /// Adds a `LessThanOrEqual` constraint `v1 <= v2 + offset`.
    pub fn add_less_than_or_equal(&mut self, v1: VariableId, v2: VariableId, offset: i64) {
        self.graph
            .add_constraint(Arc::new(LessThanOrEqual::new(v1, v2, offset)));
    }

    /// Adds a `Precedence` constraint `end(A) + min_delay <= start(B)`.
    pub fn add_precedence(&mut self, interval_a: &Interval, interval_b: &Interval, min_delay: i64) {
        self.graph
            .add_constraint(Arc::new(Precedence::new(interval_a, interval_b, min_delay)));
    }

    /// Adds an `AllowedValues` domain restriction constraint.
    pub fn add_allowed_values(&mut self, var: VariableId, allowed: impl IntoIterator<Item = i64>) {
        self.graph
            .add_constraint(Arc::new(AllowedValues::new(var, allowed)));
    }

    /// Adds a `ForbiddenValues` domain restriction constraint.
    pub fn add_forbidden_values(
        &mut self,
        var: VariableId,
        forbidden: impl IntoIterator<Item = i64>,
    ) -> ConstraintId {
        self.graph
            .add_constraint(Arc::new(ForbiddenValues::new(var, forbidden)))
    }

    /// Adds a calendar restriction: `var` may not take any value inside `unavailable_ranges`
    /// (each `(start, end)` is inclusive on both ends, e.g. "not available during `[8, 16]`").
    ///
    /// Convenience over [`Self::add_forbidden_values`] — expands each range into its individual
    /// values, it does not introduce a new constraint type or a compact periodic representation.
    /// Fine for the small, bounded horizons this crate's scheduling examples use; a calendar with
    /// a long horizon and a recurring pattern (e.g. "closed every weekend for a year") would
    /// generate one forbidden value per excluded time unit — expensive in both the constraint's
    /// own memory (`HashSet<i64>`) and matching-cost per `propagate()` call. A genuinely compact
    /// periodic calendar would need a new `Domain`/`Constraint` variant; out of scope here, see
    /// `plan/12-scheduling-vertical.md`, section 3a.
    ///
    /// # Complexity
    /// Time & Space: O(sum of range lengths) to expand the ranges into individual values.
    pub fn add_calendar(
        &mut self,
        var: VariableId,
        unavailable_ranges: &[(i64, i64)],
    ) -> ConstraintId {
        let forbidden = unavailable_ranges
            .iter()
            .flat_map(|&(start, end)| start..=end);
        self.add_forbidden_values(var, forbidden)
    }

    /// Adds a compact periodic calendar restriction without expanding excluded values across the
    /// modeled horizon. `allowed_offsets` are residues in `0..period`; absolute inclusive
    /// `unavailable_ranges` model holidays and other exceptions.
    pub fn add_periodic_calendar(
        &mut self,
        var: VariableId,
        period: i64,
        allowed_offsets: impl IntoIterator<Item = i64>,
        unavailable_ranges: impl IntoIterator<Item = (i64, i64)>,
    ) {
        self.graph.add_constraint(Arc::new(PeriodicValues::new(
            var,
            period,
            allowed_offsets,
            unavailable_ranges,
        )));
    }

    /// Adds an `AllDifferent` constraint across the given variables.
    pub fn add_all_different(&mut self, vars: impl IntoIterator<Item = VariableId>) {
        self.graph.add_constraint(Arc::new(AllDifferent::new(vars)));
    }

    /// Adds an `ExactlyOne` constraint enforcing exactly one variable takes `target_value`.
    pub fn add_exactly_one(
        &mut self,
        vars: impl IntoIterator<Item = VariableId>,
        target_value: i64,
    ) {
        self.graph
            .add_constraint(Arc::new(ExactlyOne::new(vars, target_value)));
    }

    /// Adds an `AtMost` constraint enforcing at most `k` variables take `target_value`.
    pub fn add_at_most(
        &mut self,
        k: usize,
        vars: impl IntoIterator<Item = VariableId>,
        target_value: i64,
    ) {
        self.graph
            .add_constraint(Arc::new(AtMost::new(k, vars, target_value)));
    }

    /// Adds an `AtLeast` constraint enforcing at least `k` variables take `target_value`.
    pub fn add_at_least(
        &mut self,
        k: usize,
        vars: impl IntoIterator<Item = VariableId>,
        target_value: i64,
    ) {
        self.graph
            .add_constraint(Arc::new(AtLeast::new(k, vars, target_value)));
    }

    /// Adds a `NoOverlap` constraint across intervals with fixed durations.
    pub fn add_no_overlap(&mut self, intervals: &[Interval], durations: &[u64]) {
        self.graph
            .add_constraint(Arc::new(NoOverlap::from_intervals(intervals, durations)));
    }

    /// Adds a `Cumulative` constraint over task demands and total resource capacity.
    pub fn add_cumulative(&mut self, tasks: Vec<TaskDemand>, capacity: u32) {
        self.graph
            .add_constraint(Arc::new(Cumulative::new(tasks, capacity)));
    }

    /// Adds a [`MaximumBucketLoad`] cap: the summed occupied time of `tasks` inside every
    /// [`BucketRange`] must stay within `limit`. Unlike [`Self::add_cumulative`] the cap applies to
    /// a whole bucket (e.g. one day) rather than to each instant.
    pub fn add_maximum_bucket_load(
        &mut self,
        tasks: impl IntoIterator<Item = BucketedTask>,
        ranges: impl IntoIterator<Item = BucketRange>,
        limit: i64,
    ) {
        self.graph
            .add_constraint(Arc::new(MaximumBucketLoad::new(tasks, ranges, limit)));
    }

    /// Adds a [`MinimumDistance`] constraint `|first - second| >= min_distance`.
    pub fn add_minimum_distance(
        &mut self,
        first: VariableId,
        second: VariableId,
        min_distance: i64,
    ) {
        self.graph
            .add_constraint(Arc::new(MinimumDistance::new(first, second, min_distance)));
    }

    /// Adds a [`BucketBlockPattern`] constraint: the consecutive blocks the `tasks` occupy inside
    /// the [`BucketRange`]s must form one of `allowed`, order irrelevant.
    ///
    /// Unlike [`Self::add_maximum_bucket_load`], which caps the *summed* load per bucket, this
    /// constrains the *shape* — `[2, 1, 1]` means one block of two plus two single blocks, however
    /// the buckets are distributed.
    pub fn add_bucket_block_pattern(
        &mut self,
        tasks: impl IntoIterator<Item = BucketedTask>,
        ranges: impl IntoIterator<Item = BucketRange>,
        allowed: impl IntoIterator<Item = Vec<i64>>,
    ) {
        self.graph
            .add_constraint(Arc::new(BucketBlockPattern::new(tasks, ranges, allowed)));
    }

    /// Adds a custom soft objective term to the model.
    pub fn add_objective(&mut self, objective: Arc<dyn Objective>) {
        self.graph.add_objective(objective);
    }

    /// Adds a named objective at a lexicographic soft-score level.
    pub fn add_scored_objective(
        &mut self,
        category: impl Into<String>,
        level: ScoreLevel,
        objective: Arc<dyn Objective>,
    ) {
        self.graph.add_objective(Arc::new(CategorizedObjective::new(
            category, level, objective,
        )));
    }

    /// Adds a soft objective maximizing `sum(vars) * weight` (`weight` must be positive).
    pub fn add_maximize(&mut self, vars: impl IntoIterator<Item = VariableId>, weight: i64) {
        self.graph
            .add_objective(Arc::new(WeightedSum::new(vars, weight)));
    }

    /// Adds a soft objective minimizing `sum(vars) * weight` (`weight` must be positive).
    pub fn add_minimize(&mut self, vars: impl IntoIterator<Item = VariableId>, weight: i64) {
        // `saturating_neg`: `weight == i64::MIN` has no representable negation.
        self.graph
            .add_objective(Arc::new(WeightedSum::new(vars, weight.saturating_neg())));
    }

    /// Adds a tardiness-minimization objective: for each `(end, deadline)` pair, a fresh
    /// non-negative tardiness variable `T` is introduced with `T >= end - deadline` (plus its own
    /// `T >= 0` domain floor), linearizing `T = max(0, end - deadline)` — the same trick a
    /// makespan variable uses (`end_i <= makespan` per activity, then minimize `makespan`), just
    /// with a per-pair floor instead of a shared one. In any optimal solution the solver will
    /// push `T` down to exactly `max(0, end - deadline)`, since a larger `T` only worsens a
    /// minimized objective and neither lower bound allows going below it.
    ///
    /// Returns the tardiness variables (`ends_and_deadlines.len()` of them, same order), in case
    /// the caller wants to inspect them post-solve (e.g. to report which activities ran late).
    ///
    /// # Complexity
    /// Time & Space: O(N) where N = `ends_and_deadlines.len()`.
    pub fn add_tardiness_minimize(
        &mut self,
        ends_and_deadlines: &[(VariableId, i64)],
        weight: i64,
    ) -> Vec<VariableId> {
        let mut tardiness_vars = Vec::with_capacity(ends_and_deadlines.len());
        for &(end, deadline) in ends_and_deadlines {
            let end_max = self
                .graph
                .domains()
                .get(&end)
                .and_then(Domain::max)
                .unwrap_or(deadline);
            let upper_bound = (end_max - deadline).max(0);
            let tardiness = self.new_var(format!("tardiness_{end}"), 0..=upper_bound);
            // tardiness >= end - deadline  <=>  end <= tardiness + deadline
            self.graph
                .add_constraint(Arc::new(LessThanOrEqual::new(end, tardiness, deadline)));
            tardiness_vars.push(tardiness);
        }
        self.add_minimize(tardiness_vars.clone(), weight);
        tardiness_vars
    }

    /// Compiles a scheduling model — [`Activity`]/[`Resource`]/[`Group`] — into the constraint
    /// graph: a [`Cumulative`] (multi-capacity) or [`NoOverlap`] (unary, `capacity == 1`)
    /// constraint per resource, over every activity that requires it via
    /// [`Activity::require_resource`]; two [`LessThanOrEqual`] constraints per group member,
    /// confining its interval within the group's `shared_interval`.
    ///
    /// Before this method, `Activity`/`Resource`/`Group` were pure data holders with no
    /// connection to the constraint model — callers had to manually build [`TaskDemand`]/
    /// [`TaskInterval`] themselves (see `examples/scheduling_demo.rs`'s pre-2026-09-01 version).
    /// See `plan/12-scheduling-vertical.md`, section 2.
    ///
    /// Resources or groups with no matching activities are silently skipped (nothing to
    /// constrain). Each `Interval` must have a [`DurationSpec::Fixed`] duration to participate in
    /// a resource constraint — `Cumulative`/`NoOverlap` don't support a variable duration; an
    /// activity with `DurationSpec::Variable` requiring a resource is reported as an error rather
    /// than silently dropped or mismodeled.
    ///
    /// # Errors
    /// One [`ModelError::InvalidConstraint`] per: an activity requiring an unregistered
    /// [`ResourceId`], an activity with a variable-duration interval requiring a resource, or a
    /// group referencing an unregistered [`ActivityId`]. Does not consume `self` — callers can
    /// fix the offending inputs and retry, unlike [`Self::build`].
    ///
    /// On success, returns the exact constraint generated for each used resource. Resources
    /// without matching activity demands are absent from the map. Consumers that attach domain
    /// metadata to violations should use this mapping instead of relying on constraint insertion
    /// order.
    ///
    /// # Complexity
    /// Time: O(A + D + G*M) where A = `activities.len()`, D = total resource demands across all
    /// activities, G = `groups.len()`, M = average group size.
    pub fn compile_scheduling_model(
        &mut self,
        activities: &[Activity],
        resources: &[Resource],
        groups: &[Group],
    ) -> Result<HashMap<ResourceId, ConstraintId>, Vec<ModelError>> {
        let mut errors = Vec::new();
        let mut resource_constraints = HashMap::new();
        let resource_by_id: HashMap<ResourceId, &Resource> =
            resources.iter().map(|r| (r.id(), r)).collect();
        let activity_by_id: HashMap<ActivityId, &Activity> =
            activities.iter().map(|a| (a.id(), a)).collect();

        let mut demands_by_resource: HashMap<ResourceId, Vec<(&Activity, u32)>> = HashMap::new();
        for activity in activities {
            if activity.demands().is_empty() {
                continue;
            }
            if !matches!(activity.interval().duration(), DurationSpec::Fixed(_)) {
                errors.push(ModelError::InvalidConstraint {
                    name: format!("Activity({})", activity.name()),
                    reason: "has a variable-duration interval, which Cumulative/NoOverlap don't \
                              support"
                        .to_string(),
                });
                continue;
            }
            for demand in activity.demands() {
                if !resource_by_id.contains_key(&demand.resource_id) {
                    errors.push(ModelError::InvalidConstraint {
                        name: format!("Activity({})", activity.name()),
                        reason: format!("requires unregistered resource {:?}", demand.resource_id),
                    });
                    continue;
                }
                demands_by_resource
                    .entry(demand.resource_id)
                    .or_default()
                    .push((activity, demand.demand));
            }
        }

        for resource in resources {
            let Some(demands) = demands_by_resource.get(&resource.id()) else {
                continue;
            };
            if resource.is_unary() {
                let tasks: Vec<TaskInterval> = demands
                    .iter()
                    .map(|(activity, _)| TaskInterval {
                        start: activity.interval().start(),
                        duration: match activity.interval().duration() {
                            DurationSpec::Fixed(d) => d,
                            DurationSpec::Variable(_) => unreachable!(
                                "filtered out above: only Fixed durations reach this point"
                            ),
                        },
                    })
                    .collect();
                let constraint_id = self.graph.add_constraint(Arc::new(NoOverlap::new(tasks)));
                resource_constraints.insert(resource.id(), constraint_id);
            } else {
                let tasks: Vec<TaskDemand> = demands
                    .iter()
                    .map(|(activity, demand)| TaskDemand {
                        start: activity.interval().start(),
                        duration: match activity.interval().duration() {
                            DurationSpec::Fixed(d) => d,
                            DurationSpec::Variable(_) => unreachable!(
                                "filtered out above: only Fixed durations reach this point"
                            ),
                        },
                        demand: *demand,
                    })
                    .collect();
                let constraint_id = self
                    .graph
                    .add_constraint(Arc::new(Cumulative::new(tasks, resource.capacity())));
                resource_constraints.insert(resource.id(), constraint_id);
            }
        }

        for group in groups {
            for &member_id in group.member_activities() {
                let Some(&activity) = activity_by_id.get(&member_id) else {
                    errors.push(ModelError::InvalidConstraint {
                        name: format!("Group({})", group.name()),
                        reason: format!("references unregistered activity {member_id:?}"),
                    });
                    continue;
                };
                self.graph.add_constraint(Arc::new(LessThanOrEqual::new(
                    group.shared_interval().start(),
                    activity.interval().start(),
                    0,
                )));
                self.graph.add_constraint(Arc::new(LessThanOrEqual::new(
                    activity.interval().end(),
                    group.shared_interval().end(),
                    0,
                )));
            }
        }

        if errors.is_empty() {
            Ok(resource_constraints)
        } else {
            Err(errors)
        }
    }

    /// Lets `activity` choose exactly one of `candidates` (a `(resource, demand)` pair per
    /// candidate) to run on, instead of requiring a fixed resource. Introduces one presence
    /// variable per candidate (via [`Self::new_presence_var`]) tied together by
    /// [`Self::add_exactly_one`], and — for each candidate resource — an [`Optional`]-gated
    /// pairwise non-overlap/demand constraint between `activity` and every *other* activity in
    /// `all_activities` that also requires that resource (via [`Activity::require_resource`]).
    /// Returns the presence variables, one per candidate, same order as `candidates`.
    ///
    /// # Scope and preconditions
    /// - `activity` must **not** also call [`Activity::require_resource`] for any resource in
    ///   `candidates` — that would additionally make [`Self::compile_scheduling_model`] treat the
    ///   same resource as *mandatory* for `activity`, double-constraining it. Pass the candidate
    ///   demand only through `candidates` here.
    /// - Every *other* activity in `all_activities` that requires one of `candidates`' resources
    ///   is assumed **unconditionally present** on it (i.e. compiled as a normal, non-optional
    ///   participant via [`Self::compile_scheduling_model`]). If two *different* activities both
    ///   call `add_alternative_resources` with overlapping candidate sets, the interaction
    ///   between them on a shared candidate resource is **not modeled** — treating one as
    ///   unconditionally present when it's actually also optional would be unsound (could allow
    ///   a real overlap through). Call this at most once per resource-sharing group of flexible
    ///   activities, or model such cases by hand. A fully general version would need
    ///   [`Cumulative`]/[`NoOverlap`] to natively support a per-task presence variable instead of
    ///   this pairwise composition — larger change, not attempted here (see
    ///   `plan/12-scheduling-vertical.md`, section 3c).
    /// - Every involved interval (`activity` and each `other`) must have a
    ///   [`crate::model::interval::DurationSpec::Fixed`] duration, same restriction as
    ///   [`Self::compile_scheduling_model`].
    ///
    /// # Errors
    /// One [`ModelError::InvalidConstraint`] per unregistered candidate [`ResourceId`], or per
    /// involved activity with a variable-duration interval.
    ///
    /// # Complexity
    /// Time: O(candidates.len() * all_activities.len()) to find, per candidate resource, every
    /// other activity requiring it.
    pub fn add_alternative_resources(
        &mut self,
        activity: &Activity,
        candidates: &[(ResourceId, u32)],
        all_activities: &[Activity],
        resources: &[Resource],
    ) -> Result<Vec<VariableId>, Vec<ModelError>> {
        let mut errors = Vec::new();
        let resource_by_id: HashMap<ResourceId, &Resource> =
            resources.iter().map(|r| (r.id(), r)).collect();

        let activity_duration = match activity.interval().duration() {
            DurationSpec::Fixed(d) => Some(d),
            DurationSpec::Variable(_) => {
                errors.push(ModelError::InvalidConstraint {
                    name: format!("Activity({})", activity.name()),
                    reason: "has a variable-duration interval, which alternative resources don't \
                              support"
                        .to_string(),
                });
                None
            }
        };

        let mut presences = Vec::with_capacity(candidates.len());
        for &(resource_id, _demand) in candidates {
            if !resource_by_id.contains_key(&resource_id) {
                errors.push(ModelError::InvalidConstraint {
                    name: format!("Activity({})", activity.name()),
                    reason: format!("alternative references unregistered resource {resource_id:?}"),
                });
            }
            presences.push(self.new_presence_var(format!(
                "{}_on_{:?}",
                activity.name(),
                resource_id
            )));
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        let activity_duration = activity_duration.expect("checked via errors.is_empty() above");
        self.add_exactly_one(presences.clone(), 1);

        for (&(resource_id, demand), &presence) in candidates.iter().zip(&presences) {
            let Some(&resource) = resource_by_id.get(&resource_id) else {
                continue; // already reported above
            };
            let others: Vec<&Activity> = all_activities
                .iter()
                .filter(|a| a.id() != activity.id())
                .filter(|a| a.demands().iter().any(|d| d.resource_id == resource_id))
                .collect();

            let mut other_durations = Vec::with_capacity(others.len());
            let mut duration_error = false;
            for &other in &others {
                match other.interval().duration() {
                    DurationSpec::Fixed(d) => other_durations.push(d),
                    DurationSpec::Variable(_) => {
                        errors.push(ModelError::InvalidConstraint {
                            name: format!("Activity({})", other.name()),
                            reason: "has a variable-duration interval, which alternative \
                                      resources don't support"
                                .to_string(),
                        });
                        duration_error = true;
                    }
                }
            }
            if duration_error {
                continue;
            }

            // One combined constraint over `activity` plus every mandatory `other` participant
            // on this candidate resource, gated by a single `Optional` wrap — not one pairwise
            // constraint per `other`. Pairwise capacity checks are unsound for a `Cumulative`
            // (capacity > 1) resource once 3+ tasks can coincide (no single pair exceeds
            // capacity even though all three together do); one N-way constraint checks the real
            // combined capacity directly. `others`-among-themselves feasibility is already
            // guaranteed by `compile_scheduling_model`'s own (unconditional) constraint for that
            // resource, so gating only `activity`'s participation on `presence` is sufficient —
            // whether `activity` is present or not, `others` alone are already known feasible.
            let constraint: Arc<dyn Constraint> = if resource.is_unary() {
                let mut tasks = vec![TaskInterval {
                    start: activity.interval().start(),
                    duration: activity_duration,
                }];
                tasks.extend(
                    others
                        .iter()
                        .zip(&other_durations)
                        .map(|(o, &d)| TaskInterval {
                            start: o.interval().start(),
                            duration: d,
                        }),
                );
                Arc::new(NoOverlap::new(tasks))
            } else {
                let mut tasks = vec![TaskDemand {
                    start: activity.interval().start(),
                    duration: activity_duration,
                    demand,
                }];
                tasks.extend(others.iter().zip(&other_durations).map(|(o, &d)| {
                    TaskDemand {
                        start: o.interval().start(),
                        duration: d,
                        demand: o
                            .demands()
                            .iter()
                            .find(|dem| dem.resource_id == resource_id)
                            .map(|dem| dem.demand)
                            .unwrap_or(0),
                    }
                }));
                Arc::new(Cumulative::new(tasks, resource.capacity()))
            };
            self.add_optional(constraint, presence);
        }

        if errors.is_empty() {
            Ok(presences)
        } else {
            Err(errors)
        }
    }

    /// Consumes the builder, validates the model, and returns a [`ValidatedGraph`] that public
    /// solvers accept.
    ///
    /// See [`ConstraintGraph::validate`] for what is checked.
    pub fn build(self) -> Result<ValidatedGraph, Vec<ModelError>> {
        self.graph.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{BacktrackingSolver, BranchAndBoundSolver, SolverOptions};

    #[test]
    fn test_add_calendar_forbids_ranges_and_keeps_other_values() {
        let mut builder = ModelBuilder::new();
        let var = builder.new_var("v", 0..=10);
        builder.add_calendar(var, &[(2, 4), (7, 8)]);
        let graph = builder.build().expect("model should validate");

        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome
            .solution
            .expect("feasible: values outside forbidden ranges remain");
        let val = solution.assignment[&var];
        assert!(
            !(2..=4).contains(&val) && !(7..=8).contains(&val),
            "solver picked a value inside a forbidden calendar range: {val}"
        );
    }

    #[test]
    fn test_periodic_calendar_applies_cycle_and_exception() {
        let mut builder = ModelBuilder::new();
        let var = builder.new_var("slot", 0..=20);
        builder.add_periodic_calendar(var, 10, [1], [(1, 1)]);
        let graph = builder.build().unwrap();
        let outcome = crate::solver::BacktrackingSolver::new()
            .solve(&graph, &crate::solver::SolverOptions::default());
        assert_eq!(outcome.solution.unwrap().assignment[&var], 11);
    }

    #[test]
    fn test_add_alternative_resources_avoids_forced_conflict() {
        let mut builder = ModelBuilder::new();
        let r1 = builder.new_resource("r1", 1);
        let r2 = builder.new_resource("r2", 1);

        let f1_interval = builder.new_interval("f1", 0..=0, 2, 2..=2);
        let mut f1 = builder.new_activity("f1", f1_interval);
        f1.require_resource(r1.id(), 1);

        // `fa` has the exact same fixed slot as `f1` -- choosing r1 is always infeasible.
        let fa_interval = builder.new_interval("fa", 0..=0, 2, 2..=2);
        let fa = builder.new_activity("fa", fa_interval);

        let resources = [r1, r2];
        builder
            .compile_scheduling_model(std::slice::from_ref(&f1), &resources, &[])
            .expect("valid scheduling model");

        let all_activities = [f1.clone(), fa.clone()];
        let presences = builder
            .add_alternative_resources(
                &fa,
                &[(resources[0].id(), 1), (resources[1].id(), 1)],
                &all_activities,
                &resources,
            )
            .expect("valid alternative-resource model");

        let graph = builder.build().expect("model should validate");
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome
            .solution
            .expect("feasible: fa can choose r2 instead of r1");

        assert_eq!(
            solution.assignment[&presences[0]], 0,
            "r1 would force fa to overlap f1's identical fixed slot"
        );
        assert_eq!(
            solution.assignment[&presences[1]], 1,
            "r2 is free, so fa must choose it"
        );
    }

    /// Regression test: with a `capacity == 2` candidate resource already at exactly capacity
    /// from two *other* mandatory activities (demand 1 each, no single pair exceeds capacity),
    /// adding `fa` (demand 1) to that same slot pushes total demand to 3 > 2 -- only detectable
    /// by checking all three together, not any pair alone. An earlier pairwise-decomposition
    /// implementation missed this (each pair checked in isolation always saw demand `1+1=2 <= 2`
    /// and never flagged a conflict), which would have let `fa` incorrectly "fit" on the
    /// already-full resource.
    #[test]
    fn test_add_alternative_resources_detects_combined_capacity_overload() {
        let mut builder = ModelBuilder::new();
        let hall = builder.new_resource("hall", 2);
        let hall2 = builder.new_resource("hall2", 2);

        let m1_interval = builder.new_interval("m1", 0..=0, 2, 2..=2);
        let mut m1 = builder.new_activity("m1", m1_interval);
        m1.require_resource(hall.id(), 1);

        let m2_interval = builder.new_interval("m2", 0..=0, 2, 2..=2);
        let mut m2 = builder.new_activity("m2", m2_interval);
        m2.require_resource(hall.id(), 1);

        let resources = [hall, hall2];
        builder
            .compile_scheduling_model(&[m1.clone(), m2.clone()], &resources, &[])
            .expect("valid scheduling model: m1 + m2 alone exactly fill hall's capacity 2");

        // `fa` is pinned to the exact same slot as m1/m2 -- choosing `hall` must be infeasible
        // (combined demand 3 > capacity 2), leaving `hall2` as the only option.
        let fa_interval = builder.new_interval("fa", 0..=0, 2, 2..=2);
        let fa = builder.new_activity("fa", fa_interval);

        let all_activities = [m1.clone(), m2.clone(), fa.clone()];
        let presences = builder
            .add_alternative_resources(
                &fa,
                &[(resources[0].id(), 1), (resources[1].id(), 1)],
                &all_activities,
                &resources,
            )
            .expect("valid alternative-resource model");

        let graph = builder.build().expect("model should validate");
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome
            .solution
            .expect("feasible: fa can choose hall2 instead of the already-full hall");

        assert_eq!(
            solution.assignment[&presences[0]], 0,
            "hall is already at capacity 2 from m1+m2 alone; adding fa would need capacity 3"
        );
        assert_eq!(
            solution.assignment[&presences[1]], 1,
            "hall2 is completely free, so fa must choose it"
        );
    }

    #[test]
    fn test_add_alternative_resources_reports_unknown_resource() {
        let mut builder = ModelBuilder::new();
        let interval = builder.new_interval("fa", 0..=0, 2, 2..=2);
        let fa = builder.new_activity("fa", interval);
        let all_activities = [fa.clone()];

        let result =
            builder.add_alternative_resources(&fa, &[(ResourceId(999), 1)], &all_activities, &[]);
        assert!(
            result.is_err(),
            "alternative references a resource that was never registered"
        );
    }

    #[test]
    fn test_add_optional_forced_present_delegates_to_inner() {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", 5..=5);
        let y = builder.new_var("y", 5..=5);
        let presence = builder.new_presence_var("presence");
        builder.add_allowed_values(presence, [1]); // force present
        builder.add_optional(Arc::new(NotEqual::new(x, y)), presence);

        let graph = builder.build().expect("model should validate");
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        assert!(
            outcome.solution.is_none(),
            "x and y are both pinned to 5 and NotEqual is forced present: infeasible"
        );
    }

    #[test]
    fn test_add_optional_forced_absent_ignores_inner() {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", 5..=5);
        let y = builder.new_var("y", 5..=5);
        let presence = builder.new_presence_var("presence");
        builder.add_allowed_values(presence, [0]); // force absent
        builder.add_optional(Arc::new(NotEqual::new(x, y)), presence);

        let graph = builder.build().expect("model should validate");
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        assert!(
            outcome.solution.is_some(),
            "x and y are both pinned to 5, but NotEqual is forced absent: feasible regardless"
        );
    }

    #[test]
    fn test_add_tardiness_minimize_computes_correct_tardiness() {
        let mut builder = ModelBuilder::new();
        let late_end = builder.new_var("late_end", 5..=10); // always >= 5
        let on_time_end = builder.new_var("on_time_end", 0..=2); // always <= 2
        let tardiness_vars = builder.add_tardiness_minimize(&[(late_end, 3), (on_time_end, 5)], 1);
        let graph = builder.build().expect("model should validate");

        let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome.solution.expect("feasible");
        assert_eq!(
            solution.assignment[&tardiness_vars[0]], 2,
            "late_end's minimum (5) exceeds its deadline (3) by 2, and minimizing tardiness \
             pushes late_end down to that minimum"
        );
        assert_eq!(
            solution.assignment[&tardiness_vars[1]], 0,
            "on_time_end (max 2) never exceeds its deadline (5)"
        );
    }

    /// A unary resource (`capacity == 1`) compiles to `NoOverlap`: two activities requiring it
    /// must not overlap, even though nothing in the model says so directly.
    #[test]
    fn test_compile_scheduling_model_unary_resource_forbids_overlap() {
        let mut builder = ModelBuilder::new();
        let room = builder.new_resource("room", 1);

        let interval_a = builder.new_interval("a", 0..=3, 2, 0..=5);
        let mut lesson_a = builder.new_activity("lesson_a", interval_a);
        lesson_a.require_resource(room.id(), 1);
        let interval_b = builder.new_interval("b", 0..=3, 2, 0..=5);
        let mut lesson_b = builder.new_activity("lesson_b", interval_b);
        lesson_b.require_resource(room.id(), 1);

        let resource_constraints = builder
            .compile_scheduling_model(&[lesson_a.clone(), lesson_b.clone()], &[room], &[])
            .expect("valid scheduling model");
        assert_eq!(resource_constraints.len(), 1);
        let room_constraint = resource_constraints[&lesson_a.demands()[0].resource_id];
        let graph = builder.build().expect("model should validate");
        assert_eq!(
            graph.get_constraint(room_constraint).unwrap().name(),
            "NoOverlap"
        );

        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome.solution.expect("feasible: rooms can be staggered");
        let start_a = solution.assignment[&lesson_a.interval().start()];
        let start_b = solution.assignment[&lesson_b.interval().start()];
        assert!(
            (start_a - start_b).abs() >= 2,
            "lessons on a unary resource must not overlap: {start_a} vs {start_b}"
        );
    }

    /// A multi-capacity resource compiles to `Cumulative`: total demand at any instant must stay
    /// within capacity, but activities *can* overlap as long as they fit.
    #[test]
    fn test_compile_scheduling_model_cumulative_resource_allows_overlap_within_capacity() {
        let mut builder = ModelBuilder::new();
        let room = builder.new_resource("hall", 2);

        let interval_a = builder.new_interval("a", 0..=0, 2, 0..=2);
        let mut a = builder.new_activity("a", interval_a);
        a.require_resource(room.id(), 1);
        let interval_b = builder.new_interval("b", 0..=0, 2, 0..=2);
        let mut b = builder.new_activity("b", interval_b);
        b.require_resource(room.id(), 1);

        builder
            .compile_scheduling_model(&[a.clone(), b.clone()], &[room], &[])
            .expect("valid scheduling model");
        let graph = builder.build().expect("model should validate");

        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        assert!(
            outcome.solution.is_some(),
            "two demand-1 activities on a capacity-2 resource, both pinned to start 0, must be \
             jointly feasible (they may overlap)"
        );
    }

    /// A group's `shared_interval` confines every member: a member's interval must fit within
    /// it, even when the member's own domain would otherwise allow escaping it.
    #[test]
    fn test_compile_scheduling_model_group_confines_members() {
        let mut builder = ModelBuilder::new();
        let group_interval = builder.new_interval("g", 5..=5, 3, 8..=8); // fixed [5,8)
        let mut group = builder.new_group("g", group_interval);

        let member_interval = builder.new_interval("m", 0..=10, 2, 0..=12);
        let member = builder.new_activity("m", member_interval);
        group.add_member(member.id());

        builder
            .compile_scheduling_model(std::slice::from_ref(&member), &[], &[group])
            .expect("valid scheduling model");
        let graph = builder.build().expect("model should validate");

        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        let solution = outcome
            .solution
            .expect("feasible: member fits within [5,8)");
        let start = solution.assignment[&member.interval().start()];
        let end = solution.assignment[&member.interval().end()];
        assert!(
            start >= 5 && end <= 8,
            "member interval [{start},{end}) must fit within the group's shared interval [5,8)"
        );
    }

    #[test]
    fn test_compile_scheduling_model_reports_unknown_resource() {
        let mut builder = ModelBuilder::new();
        let interval = builder.new_interval("a", 0..=1, 2, 0..=3);
        let mut activity = builder.new_activity("a", interval);
        activity.require_resource(ResourceId(999), 1);

        let result = builder.compile_scheduling_model(&[activity], &[], &[]);
        assert!(
            result.is_err(),
            "activity requires a resource that was never registered"
        );
    }

    #[test]
    fn test_compile_scheduling_model_reports_unknown_activity_in_group() {
        let mut builder = ModelBuilder::new();
        let group_interval = builder.new_interval("g", 0..=0, 10, 10..=10);
        let mut group = builder.new_group("g", group_interval);
        group.add_member(ActivityId(999));

        let result = builder.compile_scheduling_model(&[], &[], &[group]);
        assert!(
            result.is_err(),
            "group references an activity that was never registered"
        );
    }

    #[test]
    fn test_compile_scheduling_model_reports_variable_duration_on_resource() {
        let mut builder = ModelBuilder::new();
        let room = builder.new_resource("room", 1);
        let duration_var = builder.new_var("dur", 1..=5);
        let start = builder.new_var("start", 0..=10);
        let end = builder.new_var("end", 0..=15);
        let interval = Interval::new(start, DurationSpec::Variable(duration_var), end);
        let mut activity = builder.new_activity("a", interval);
        activity.require_resource(room.id(), 1);

        let result = builder.compile_scheduling_model(&[activity], &[room], &[]);
        assert!(
            result.is_err(),
            "Cumulative/NoOverlap don't support a variable-duration interval"
        );
    }
}
