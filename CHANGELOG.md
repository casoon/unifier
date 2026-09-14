# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/); before 1.0, a minor version can contain breaking
changes.

Versions 0.2.0 to 0.3.1 were published to crates.io before the matching changes were committed;
the repository history records them after the fact, one commit per release.

## [Unreleased]

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
