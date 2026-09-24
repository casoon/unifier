# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/); before 1.0, a minor version can contain breaking
changes.

Versions 0.2.0 to 0.3.1 were published to crates.io before the matching changes were committed;
the repository history records them after the fact, one commit per release.

## [Unreleased]

### Changed

- `BacktrackingSolver` now asks the constraints of each newly assigned variable whether they
  can still be satisfied (`Constraint::is_satisfiable`) after propagation succeeds, and treats
  a "no" as a conflict — including the `dom/wdeg` weight increment a propagation conflict
  gets. Before, a constraint whose `propagate` reported nothing was checked at the leaf alone:
  a violation decided near the root cost a full sweep of every variable below it.
  `BranchAndBoundSolver` never had that blind spot; its bound asks every constraint through
  `optimistic_score`. This brings the tree search level with it.

  Measured: a three-variable model with one unpropagated constraint over two booleans and a
  20,001-value variable below them went from 216,614 nodes to 4
  (`tests/unpropagated_constraint.rs`). On schedulr's scale instances, whose resource-capacity
  constraint propagates only part of what it checks, the tree search went from 2,622 to 62
  nodes and from 214,742 to 82, both solutions verified independently. Every deterministic
  case in `examples/bench_search.rs` expands the same number of nodes as before; the N-Queens
  Criterion benchmark moves by +3.0 %, −1.2 % and −2.3 % for n = 8, 10, 12.

  The check is sound because `is_satisfiable` may answer `false` only when no completion can
  satisfy the constraint — the contract 0.5.1 made `ExactlyOne` and `AtLeast` keep under
  partial assignments.

## [0.5.1] - 2026-09-20

### Fixed

- `ExactlyOne` and `AtLeast` no longer report a violation under a **partial** assignment merely
  because nothing has reached the target yet. `Constraint::is_satisfied` documents that it
  answers `false` only when what is already assigned breaks the constraint — `AllDifferent`
  looks only at assigned variables, `Equal` compares only when both sides are there, `AtMost`
  counts upwards, and `BucketLoad` and `MinimumDistance` each pin the rule with a test of their
  own. These two counted `== 1` and `>= k` outright, so a group nobody had chosen in yet
  called itself broken while every variable in it could still become the one.

  On a **complete** assignment nothing changes: there is nothing unassigned, and both fall back
  to their old condition. The hard score, which sums over complete assignments, sees the same
  numbers as before — which is why this stayed invisible for so long.

  It was not cosmetic. The greedy construction in `LocalSearchSolver` places a variable without
  retracting only when *no* constraint is violated, and a variable in an untouched choice group
  could never meet that condition: it retracted, and took the variables it called "blocking"
  with it. On a timetabling instance with 231 indicator variables, construction stalled at
  ~105 violations across every seed; with the fix it reaches 0, and instances that had only
  ever hit their time limit now solve (measured in timbra's plan/58, K3).

## [0.5.0] - 2026-09-20

A release about what a search may report. Until now a run either returned a fully feasible
solution or nothing at all, and the score between those two states carried almost no
information. Both are fixed here, and the construction that feeds the search was rebuilt.

### Changed

- **Breaking:** `SolveOutcome` has a new `best_effort` field, so a struct literal listing every
  field no longer compiles. The constructors are unaffected.
- **Breaking:** the hard score sums how *badly* each constraint is broken instead of counting
  the constraints that answered "not satisfied". A `NoOverlap` over forty time points used to
  cost the same for one collision as for five, so a move that resolved a collision *inside* an
  already-broken constraint had score delta zero — invisible to tabu memory, to Branch & Bound
  as a bound, and to every acceptance rule. `is_feasible()` is still `hard == 0` and means what
  it meant; only the numbers in between changed, and any caller comparing hard scores across
  runs of different versions will see different magnitudes.
- **Breaking:** `SharedIncumbent::offer` records what it is given instead of dropping anything
  infeasible. `best()` is unchanged — still feasible-only, still what a portfolio returns — but
  callers no longer need to pre-filter, and the near miss a repair search wants now survives.
- `BacktrackingSolver` and `BranchAndBoundSolver` descend over an explicit stack on the heap
  rather than over the call stack. The depth of an assignment search is the number of variables,
  so a recursive descent overflowed on large instances — and a stack overflow aborts the process,
  leaving the caller with no result where it should have got "nothing within the budget". Two
  guards pin it: 1000 variables on a 64 KiB stack, cross-checked against the recursive version.
  Nothing gets faster from this; a large instance now reports its time limit instead of taking
  the process with it.
- `LargeNeighborhoodSearch` repairs an infeasible center with Local Search instead of asking an
  exact sub-search for a fully feasible sub-assignment. A neighbourhood that contains a violated
  constraint only in part is unsolvable by construction, and the constraints in question span
  dozens of variables — measured, not one round in thirty seconds improved anything. While the
  center is feasible the exact sub-search stays, which is the right tool for that question. Also
  fixed: an exhausted LNS loop could spend a second full time limit.
- The greedy construction in `LocalSearchSolver` takes back what stands in the way instead of
  keeping a conflicted value: when no conflict-free value exists it un-assigns the *other*
  variables in the violated constraints and re-queues them, bounded per variable so the
  procedure stays finite. Tightest domains go first, ties shuffled, and several attempts run
  with the best kept. Measured separately — conflict-directed rather than most-recent undo, and
  tightest-first ordering, each account for a large part of it: on the reference instance the
  construction went from 22 violations to none.

### Added

- `Constraint::violations`, answering how many ways a constraint is broken under an assignment —
  `0` exactly when `is_satisfied` is `true`. It has a default returning `1` per violation, so
  external implementations keep compiling and keep their old behaviour. `AllDifferent`,
  `NoOverlap`, `Cumulative`, `MaximumBucketLoad` and `BucketBlockPattern` override it; the hard
  score is the sum. Differential tests walk every assignment of a small instance per constraint
  to hold the `0` ⇔ satisfied equivalence, which feasibility now depends on.
- `SolveOutcome::best_effort` and `SolveOutcome::reached()`. `best_effort` is the best
  **complete** assignment a run reached when it could not vouch for one, present on an
  `Infeasible` outcome too; it is set only while `solution` is `None`, so the two can never be
  confused. `reached()` is the question a repair search asks: somewhere to continue from,
  regardless of how the previous run ended.
- `SharedIncumbent::center()`, the best complete assignment offered so far whether feasible or
  not. Read `center()` to decide where to work and `best()` to decide what to report.
- `LocalSearchSolver::repair_from`, which repairs a caller's assignment instead of building its
  own. A contradiction in root propagation is not a reason to stop here — a fixed assignment
  being contradictory is what "repair" means — and the narrowed domains are adopted only when
  they came out consistent.

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
