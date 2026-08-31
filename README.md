# unifier

Constraint satisfaction / optimization (CSP/COP) modeling and solver
framework for Rust, integrating with
[`pathwise`](https://github.com/casoon/pathwise)'s generic search and
optimization traits (see "Relationship to `pathwise`" below for how far
that integration currently goes).

## Status

Pre-release, not yet published to crates.io. The core model, constraint
propagation, global constraints, hard/soft scoring with weighted
objectives, fallible model validation (`ConstraintGraph::validate` /
`ModelBuilder::build`), and five solver strategies (Backtracking,
Branch & Bound with optimistic-bound pruning, Local Search, LNS,
Parallel Portfolio) are implemented and tested. `SolveOutcome` reports
status (`Optimal` / `Feasible` / `Infeasible` / `Aborted(reason)`), the
best solution found, search statistics, and — for Branch & Bound — a
score bound.

See `plan/00-STATUS.md` (local, untracked) for the implementation log,
`plan/08-project-evaluation.md` for the first independent maturity
assessment, and `plan/09-project-reevaluation-roadmap.md` for the
follow-up assessment (including a since-fixed false-optimality-proof
bug) and the maturity roadmap — read its release recommendation before
relying on this for production planning scenarios.

## Problem class

Constraint Satisfaction Problems (CSP) — and, once an objective is
optimized rather than just satisfied, Constraint Optimization Problems
(COP): variables, domains, constraints, and (for COP) an objective.
Typical instances: scheduling, timetabling, resource allocation.

These problems are generally NP-hard — there is no single "best
algorithm" the way there is for sorting. `unifier`'s solver strategies
are therefore interchangeable rather than fixed, and the design targets
an **anytime solver**: a valid solution fast, then iterative improvement,
cancellable at any point.

## Core model

- **Variable / Domain** — `Variable` (integer-valued, identified by
  `VariableId`) with a `Domain`: `Range { min, max }` for contiguous
  bounds, or `Explicit(BTreeSet<i64>)` once a value is punched out of
  the middle of a range
- **Constraint** — `Equal`, `NotEqual`, `LessThanOrEqual`,
  `AllDifferent`, `NoOverlap`, `Cumulative`, `Precedence`,
  `AllowedValues`/`ForbiddenValues`, `ExactlyOne`/`AtMost`/`AtLeast`
- **Objective** — `WeightedSum` soft-score terms (hard constraints are
  never violated in a feasible solution; soft terms are a weighted
  preference to maximize), aggregated into a `HardSoftScore`
- **Interval / Resource / Activity / Group** — scheduling-oriented data
  types (start/duration/end, capacity, resource demands, grouped
  activities sharing one interval); currently plain data objects the
  DSL wires into `Equal`/`NoOverlap`/`Cumulative` constraints, not yet a
  first-class part of the constraint graph itself (see
  `plan/09-project-reevaluation-roadmap.md`)

The problem itself is modeled as a **constraint graph** (a hypergraph of
variables, constraints, and objectives), not a tree — the tree only
emerges as part of a solver's search process. `ConstraintGraph::validate`
/ `ModelBuilder::build` reject structurally invalid models (unknown
variable references, empty domains, duplicate IDs, self-contradictory
constraint parameters) before a solver ever sees them.

## Relationship to `pathwise`

`pathwise` provides the generic `Problem`/`OptimizationProblem` traits
and interchangeable search/optimization strategies (A*, branch and
bound, local search, simulated annealing, ...). `unifier`'s
`UnifierProblemAdapter` implements those traits so a `unifier` model can
be driven by `pathwise`'s generic algorithms. Today this is a formal
bridge rather than the primary search path: `unifier` ships its own
CSP/COP-specialized solvers (constraint propagation — mostly bounds- and
singleton-consistency, not full arc consistency for every global
constraint — MRV/fail-first variable ordering, and Branch & Bound with
its own optimistic-bound pruning) rather than routing through
`pathwise`'s engines. Whether `pathwise` becomes the actual search
runtime or stays an optional integration point is an open question, see
`plan/09-project-reevaluation-roadmap.md`.

See `plan/01-concept.md` for the full architecture (4 layers: DSL,
constraint model, solver engine, runtime) and the MVP scope for 0.1.

## Installation

Not yet published to crates.io.

## License

MIT — see [LICENSE](LICENSE).
