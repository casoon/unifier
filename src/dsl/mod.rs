//! High-level DSL surface API for building CSP/COP models.
//!
//! Provides fluent builders for creating variables, domains, intervals, resources,
//! activities, and constraint specifications.

use crate::constraint::{
    AllDifferent, AllowedValues, AtLeast, AtMost, Constraint, Cumulative, Equal, ExactlyOne,
    ForbiddenValues, LessThanOrEqual, NoOverlap, NotEqual, Precedence, TaskDemand,
};
use crate::model::activity::{Activity, ActivityId};
use crate::model::domain::Domain;
use crate::model::group::{Group, GroupId};
use crate::model::interval::{DurationSpec, Interval};
use crate::model::resource::{Resource, ResourceId};
use crate::model::variable::{Variable, VariableId};
use crate::propagation::graph::{ConstraintGraph, ModelError, ValidatedGraph};
use crate::score::{Objective, WeightedSum};
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
    pub fn add_constraint(&mut self, constraint: Arc<dyn Constraint>) {
        self.graph.add_constraint(constraint);
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
    ) {
        self.graph
            .add_constraint(Arc::new(ForbiddenValues::new(var, forbidden)));
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

    /// Adds a custom soft objective term to the model.
    pub fn add_objective(&mut self, objective: Arc<dyn Objective>) {
        self.graph.add_objective(objective);
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

    /// Consumes the builder, validates the model, and returns a [`ValidatedGraph`] that public
    /// solvers accept.
    ///
    /// See [`ConstraintGraph::validate`] for what is checked.
    pub fn build(self) -> Result<ValidatedGraph, Vec<ModelError>> {
        self.graph.finalize()
    }
}
