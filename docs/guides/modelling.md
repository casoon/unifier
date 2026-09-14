---
title: Modelling
description: Variables, domains, constraints and objectives, the scheduling primitives, and how a model is validated.
order: 1
---

A unifier model is a **constraint graph**: a hypergraph whose nodes are variables and whose
edges are constraints and objectives. It is not a tree; a tree only appears while a solver
searches. You build the graph with `ModelBuilder` and finish it with `build()`, which validates
it and returns a `ValidatedGraph` that every solver accepts.

## Variables and domains

Every variable is an integer with a domain of allowed values:

```rust
let mut model = ModelBuilder::new();
let slot = model.new_var("slot", 0..=7);       // Domain::Range { min: 0, max: 7 }
let used = model.new_presence_var("used");     // domain 0..=1, for optional parts
```

A domain starts as a contiguous `Range { min, max }` and becomes an `Explicit` set once a value
is removed from the middle.

## Constraints

Hard constraints must hold in every feasible solution. The builder has a method for each
built-in constraint:

| Constraint | Builder method | Meaning |
| --- | --- | --- |
| `Equal` | `add_equal(v1, v2, offset)` | `v1 = v2 + offset` |
| `NotEqual` | `add_not_equal(v1, v2)` | `v1 != v2`; `NotEqual::with_offset` adds an offset |
| `LessThanOrEqual` | `add_less_than_or_equal(v1, v2, offset)` | `v1 <= v2 + offset` |
| `AllDifferent` | `add_all_different(vars)` | all values pairwise distinct |
| `AllowedValues` | `add_allowed_values(var, values)` | `var` takes one of `values` |
| `ForbiddenValues` | `add_forbidden_values(var, values)` | `var` takes none of `values` |
| `ExactlyOne` | `add_exactly_one(vars, value)` | exactly one variable equals `value` |
| `AtMost` | `add_at_most(k, vars, value)` | at most `k` variables equal `value` |
| `AtLeast` | `add_at_least(k, vars, value)` | at least `k` variables equal `value` |
| `Precedence` | `add_precedence(a, b, delay)` | `end(a) + delay <= start(b)` |
| `NoOverlap` | `add_no_overlap(intervals, durations)` | intervals never run at the same time |
| `Cumulative` | `add_cumulative(tasks, capacity)` | summed demand never exceeds `capacity` |

`add_optional(constraint, presence)` wraps any constraint so that it only applies once the
presence variable is fixed to `1`. Your own types implementing the `Constraint` trait are added
with `add_constraint`.

The global constraints have dedicated propagation: `AllDifferent` reaches generalized arc
consistency with Régin's matching algorithm, `Cumulative` and `NoOverlap` detect resource
overload by energetic reasoning, and `NoOverlap` also tightens bounds by edge-finding.

## Objectives and scores

Solutions are ranked by a `HardSoftScore`. The **hard** part is `0` when all hard constraints
hold and negative otherwise; the **soft** part is the sum of the weighted objective terms, and
larger is better. A score is compared by `hard` first, then by `soft`, so no amount of soft
score makes up for a violated hard constraint.

| Builder method | Soft term |
| --- | --- |
| `add_maximize(vars, weight)` | `+ weight × sum(vars)` |
| `add_minimize(vars, weight)` | `− weight × sum(vars)` |
| `add_tardiness_minimize(ends_and_deadlines, weight)` | `− weight × Σ max(0, end − deadline)`; returns the tardiness variables |
| `add_objective(objective)` | any type implementing `Objective` |

A feasible score prints as `Feasible(soft)`, for example `Feasible(-11)` for a minimized
makespan of 11.

## Scheduling

`new_interval(name, start_range, duration, end_range)` creates an interval: start and end
variables with the implicit constraint `end = start + duration`. On top of intervals, three data
types describe a scheduling problem:

- `Resource`: a named capacity, created with `new_resource(name, capacity)`.
- `Activity`: an interval that requires resources, created with `new_activity(name, interval)`
  and `require_resource(resource, demand)`.
- `Group`: activities that must stay inside one shared interval.

`compile_scheduling_model(activities, resources, groups)` turns these into constraints:
`Cumulative` for resources with capacity greater than 1, `NoOverlap` for unary resources, and
two `LessThanOrEqual` constraints per group member. It reports references to unknown resources
or activities as errors instead of dropping them.

Further helpers cover the usual scheduling needs:

- `add_calendar(var, ranges)` excludes time ranges, for example a blocked slot.
- `add_optional` with a presence variable models an activity that may or may not take place.
- `add_alternative_resources` lets an activity use exactly one of several resources.
- `add_tardiness_minimize` penalizes finishing after a deadline.

The [school timetable](../../../showcase/school-timetable/) in the showcase uses all of them.

## Validation

`build()` returns `Result<ValidatedGraph, Vec<ModelError>>`. It rejects a structurally broken
model before a solver sees it:

| `ModelError` | Cause |
| --- | --- |
| `UnknownVariable` | a constraint or objective refers to a variable that was never added |
| `EmptyDomain` | a variable can never be assigned |
| `DuplicateVariableId` | the same variable id was registered twice |
| `InvalidConstraint` | a constraint's own parameters contradict each other, for example a `Cumulative` demand above its capacity |

## Checking changes without a search

`ValidatedGraph::check_incremental(committed, proposed)` overlays proposed values on an existing
assignment and evaluates only the constraints that touch a changed variable. It starts no
propagation and no search, which makes it cheap enough to check an edit, for example moving one
activity, while a user makes it.

Each violation is a `ConstraintViolation` with the constraint's name, the variables involved and
an English message. `NoOverlap`, `Cumulative` and `Precedence` produce specialized
explanations; other constraints report that they are violated.
