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
| `PeriodicValues` | `add_periodic_calendar(var, period, offsets, unavailable)` | `var` falls on an allowed offset of a repeating period and outside the unavailable ranges |
| `MinimumDistance` | `add_minimum_distance(first, second, d)` | `first` and `second` are at least `d` apart |
| `MaximumBucketLoad` | `add_maximum_bucket_load(tasks, ranges, limit)` | summed load inside each bucket never exceeds `limit` |
| `BucketBlockPattern` | `add_bucket_block_pattern(tasks, ranges, allowed)` | the occupied blocks form one of the `allowed` shapes |

`PeriodicValues`, `MinimumDistance` and the bucket constraints are described in more detail [below](#periodic-calendars-distances-and-bucket-loads).

`add_optional(constraint, presence)` wraps any constraint so that it only applies once the
presence variable is fixed to `1`. Your own types implementing the `Constraint` trait are added
with `add_constraint`, which returns the new constraint's `ConstraintId`.

The global constraints have dedicated propagation: `AllDifferent` reaches generalized arc
consistency with Régin's matching algorithm, `Cumulative` and `NoOverlap` detect resource
overload by energetic reasoning, and `NoOverlap` also tightens bounds by edge-finding.

## Objectives and scores

Solutions are ranked by a `HardSoftScore`. The **hard** part is `0` when all hard constraints
hold and negative otherwise. The soft part is split into three levels, `strong`, `medium` and
`weak`, each the sum of the objective terms on that level; larger is better. `soft` holds the
sum of all three for reporting.

Scores compare lexicographically: `hard` first, then `strong`, `medium` and `weak`. No amount
of soft score makes up for a violated hard constraint, and no amount of a lower level makes up
for a worse higher level.

| Builder method | Soft term |
| --- | --- |
| `add_maximize(vars, weight)` | `+ weight × sum(vars)` |
| `add_minimize(vars, weight)` | `− weight × sum(vars)` |
| `add_tardiness_minimize(ends_and_deadlines, weight)` | `− weight × Σ max(0, end − deadline)`; returns the tardiness variables |
| `add_objective(objective)` | any type implementing `Objective` |
| `add_scored_objective(category, level, objective)` | `objective` on the given `ScoreLevel`, under a category name |

Objectives land on the `weak` level unless they say otherwise: `Objective::level()` defaults to
`ScoreLevel::Weak`. `add_scored_objective` wraps an objective in a `CategorizedObjective` that
sets the level (`ScoreLevel::Strong`, `Medium` or `Weak`) and a category, which
`Objective::category()` reports for a score drill-down. A model whose objectives all stay on
`weak` ranks solutions exactly by their summed soft score.

A feasible score prints all three levels, for example
`Feasible(strong=0, medium=0, weak=-11)` for a minimized makespan of 11. An infeasible one prints
`Infeasible(hard=…, strong=…, medium=…, weak=…)`. `HardSoftScore::new(hard, soft)` puts `soft`
on the `weak` level; `HardSoftScore::tiered(hard, strong, medium, weak)` sets every level.

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

## Periodic calendars, distances and bucket loads

These constraints carry no scheduling vocabulary of their own: a value can be a slot, a minute
or a day, and mapping a real calendar onto integer values and ranges is up to the model.

### Periodic calendars

`add_periodic_calendar(var, period, allowed_offsets, unavailable_ranges)` adds a
`PeriodicValues` constraint. A value is allowed when its remainder modulo `period` is one of
`allowed_offsets` (residues in `0..period`) and it lies in none of the absolute, inclusive
`unavailable_ranges`, which model holidays and other exceptions:

```rust
let slot = model.new_var("slot", 0..=20);
// Every 10 slots, only offset 1 is open; slot 1 itself is a holiday.
model.add_periodic_calendar(slot, 10, [1], [(1, 1)]);
// Solutions: slot = 11 (21 is outside the domain).
```

Unlike `add_calendar`, which lists every excluded value, the constraint stores only the offsets
and the exceptions, so its size does not grow with the modelled horizon. Propagation removes
disallowed values from domains of up to 4096 values and tightens only the bounds of larger ones.
`build()` rejects a period that is not positive and an empty offset list.

### Minimum distance

`add_minimum_distance(first, second, d)` adds a `MinimumDistance` constraint: the values of
`first` and `second` differ by at least `d`, in either order. A `d` of `0` or less always holds.
Which pairs need a distance, for example two lessons of the same teacher that need a break
between them, is decided by the model. As soon as one side is fixed, propagation removes the
values closer than `d` from the other side's domain. On a range domain it can only cut a prefix
or a suffix; a forbidden band strictly inside the range is left to the search.

### Bucket loads and block patterns

Two constraints look at how much of a group of tasks falls into each *bucket*, such as each day
of a week. A task is a `BucketedTask`: a start variable, an occupied length and a demand per time
unit, optionally with a presence variable (`with_presence`) so that it only counts while that
variable is `1`. A bucket is described by one or more half-open `BucketRange`s
`[start, end)`; several ranges may belong to the same bucket.

- `add_maximum_bucket_load(tasks, ranges, limit)` adds a `MaximumBucketLoad` constraint: the
  summed `demand × occupied time` inside every bucket stays within `limit`. A task that crosses a
  range boundary contributes its overlap to each side; a task outside every range counts for
  none. `Cumulative` bounds the load at each instant; this constraint bounds the total over a
  whole bucket.
- `add_bucket_block_pattern(tasks, ranges, allowed)` adds a `BucketBlockPattern` constraint.
  Inside each range, touching or overlapping tasks merge into one block; the block lengths of all
  ranges together, in any order, must equal one of the `allowed` lists. `[2, 1, 1]` means one
  block of two plus two blocks of one, however they are spread over the buckets. Demands are
  ignored here.

```rust
use unifier::{BucketRange, BucketedTask};

// Day 0 covers slots 0..8, day 1 covers slots 8..16 (half-open ranges).
let days = [BucketRange::new(0, 8, 0), BucketRange::new(8, 16, 1)];
let lessons: Vec<BucketedTask> = starts.iter().map(|&start| BucketedTask::new(start, 1, 1)).collect();

// At most three lesson slots per day.
model.add_maximum_bucket_load(lessons.clone(), days, 3);
// Across both days: one double lesson and two single lessons.
model.add_bucket_block_pattern(lessons, days, [vec![2, 1, 1]]);
```

Neither constraint filters domains; both are checked during the search and explain a violation
for `check_incremental`. A bucket load only grows as tasks are placed, so an exceeded bucket is
rejected as soon as it occurs. A block pattern can still change with every further task, so it is
only judged once every task and presence variable is fixed. `build()` rejects negative limits,
durations or demands, empty or overlapping ranges, and empty or non-positive block patterns.
`BucketBlockPattern` itself is not re-exported at the crate root; use
`unifier::constraint::BucketBlockPattern` when you need the type.

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
