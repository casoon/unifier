# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/); before 1.0, a minor version can contain breaking
changes.

Versions 0.2.0 to 0.3.1 were published to crates.io before the matching changes were committed;
the repository history records them after the fact, one commit per release.

## [Unreleased]

## [0.4.0] - 2026-09-18

A search release. The solvers were strong at proving optimality on small models and weak at
finding any solution on large ones; this evens that out.

### Changed

- **Breaking:** `SolverOptions` has a new `seed` field, so a struct literal listing every field
  no longer compiles. `..SolverOptions::default()` is unaffected.
- **Breaking:** `ParallelSolver` adopts an incumbent the caller set in
  `SolverOptions::shared_incumbent` instead of discarding it and starting empty. A caller that
  already holds a solution can hand it over, and improvements flow back into that handle.
- `BranchAndBoundSolver` orders variables by `dom/wdeg` instead of plain MRV, threading through
  its recursion the same constraint weights `BacktrackingSolver` already used, and tries values
  least-constraining first. Both solvers share that value ordering.
- Ties in the variable ordering resolve to the lowest `VariableId` rather than to `HashMap`
  iteration order, which Rust randomizes per process. Before this, five runs over one unchanged
  model split three ways solved and twice never finished, purely by hash seed — no run was
  reproducible, so no measurement of this solver meant anything.
- `LocalSearchSolver` repairs conflicts instead of rescanning the model: while hard constraints
  are violated it draws a violated constraint and moves one of its variables, scores candidates
  in place rather than cloning the assignment per candidate, and starts from AC-3 plus a greedy
  pass instead of every variable's domain minimum. On graph colouring, throughput at 240
  variables went from 5.6k to 315k moves per second and stopped falling as the model grows.
- `BacktrackingSolver` restarts on a Luby schedule, carrying its `dom/wdeg` weights across
  restarts. A run cut short by a restart budget is tracked apart from one that exhausted the
  search space, so only the latter still reports `Infeasible`.
- A search node propagates from the constraints of the variable it just assigned rather than
  from every constraint in the graph. Node counts across the benchmark corpus are unchanged to
  the digit — the same tree, reached about six times faster on a large model.

### Added

- `PropagationEngine::propagate_from`, taking the seed constraints. `propagate` is now the
  special case that seeds everything, which is still what a root-level call wants.
- `SolverOptions::seed` for the randomized tie-breaking in `LocalSearchSolver`, fixed by default
  so a run replays exactly.

### Removed

- `select_mrv_variable` (crate-internal), which lost its last caller.

## [0.3.2] - 2026-09-14

### Added

- `MinimumDistance` constraint and `ModelBuilder::add_minimum_distance`: two variables must be at
  least a given distance apart. Propagation prunes the other side once one side is fixed.
- `MaximumBucketLoad` constraint and `ModelBuilder::add_maximum_bucket_load`: caps the summed
  `demand × occupied time` of a group of `BucketedTask`s inside every bucket, described by
  half-open `BucketRange`s (for example one per day). Optional tasks count only while their
  presence variable is `1`.
- `BucketBlockPattern` constraint and `ModelBuilder::add_bucket_block_pattern`: the consecutive
  blocks a group of tasks occupies across all buckets must form one of the allowed block-length
  multisets, such as `[2, 1, 1]`.
- `BucketRange`, `BucketedTask`, `MaximumBucketLoad` and `MinimumDistance` are re-exported at the
  crate root.

## [0.3.1] - 2026-09-04

### Fixed

- `LnsSolver` no longer returns an assignment of domain minimums, which could violate
  constraints, when it has nothing to destroy or ends without a feasible solution; it falls back
  to the repair solver instead.
- `HardSoftScore`'s ordering also compares the summed `soft` score last, so it agrees with
  equality.

## [0.3.0] - 2026-09-04

### Added

- Lexicographic soft-score levels: `HardSoftScore` has `strong`, `medium` and `weak` next to the
  summed `soft`, `HardSoftScore::tiered` builds such a score, and `ScoreLevel` names the levels.
- `Objective::category` and `Objective::level` (defaults: the objective's name and
  `ScoreLevel::Weak`), `CategorizedObjective` to set both for any objective, and
  `ModelBuilder::add_scored_objective`.
- `PeriodicValues` constraint and `ModelBuilder::add_periodic_calendar`: allowed offsets in a
  repeating period plus absolute unavailable ranges, stored independently of the horizon.
- `LnsSolver::solve_from`: repair from a caller-provided baseline assignment, even one that
  changed constraints have made infeasible.

### Changed

- `ScoreCalculator` totals each soft level separately.
- `LnsSolver` only keeps feasible solutions as its best solution and shared incumbent.

### Breaking

- `HardSoftScore` has new public fields, so struct literals no longer compile; use
  `HardSoftScore::new` or `HardSoftScore::tiered`.
- `HardSoftScore` prints every level, for example `Feasible(strong=0, medium=0, weak=-11)`
  instead of `Feasible(-11)`.
- `ModelBuilder::add_constraint` returns the new `ConstraintId`.

## [0.2.0] - 2026-09-04

### Added

- `ValidatedGraph::check_incremental`: evaluates only the constraints touched by changed
  variables, without starting a search, and returns `ConstraintViolation`s.
- Structured constraint explanations (`Constraint::explain`, `Explanation`), with specialized
  messages for `NoOverlap`, `Cumulative` and `Precedence`.

### Changed

- Requires `pathwise` 0.2.0 instead of 0.1.0.

### Breaking

- `ModelBuilder::compile_scheduling_model` returns the `ResourceId` to `ConstraintId` mapping it
  built (`Result<HashMap<ResourceId, ConstraintId>, _>` instead of `Result<(), _>`), so a
  violation can be attributed to its resource.

## [0.1.1] - 2026-09-01

### Added

- Criterion benchmarks for the solvers.

### Fixed

- Stale references in the README.

## [0.1.0] - 2026-09-01

### Added

- First release: constraint graph with validation, built-in and global constraints, hard/soft
  scoring, five solvers (Backtracking, Branch & Bound, Local Search, LNS, parallel portfolio) and
  scheduling primitives.
