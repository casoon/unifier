# unifier

Constraint satisfaction / optimization (CSP/COP) modeling and solver
framework for Rust, built on
[`pathwise`](https://github.com/casoon/pathwise)'s generic search and
optimization primitives.

## Status

Concept phase. No implementation yet — see `plan/01-concept.md` (local,
untracked) for scope and design rationale.

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

## Core model (working title)

- **Variable / Domain** — `Variable<T>` with a domain (`BitSet` / `Range`
  / `SparseSet`)
- **Constraint** — `Equal`, `NotEqual`, `AllDifferent`, `NoOverlap`,
  `Cumulative`, `Precedence`, and other global constraints
- **Interval / Resource / Activity** — scheduling-specific building
  blocks (start/duration/end, capacity, resource demands), including
  grouped/composite activities that share one interval (e.g. "all
  participants of a group start and end together")
- **Objective** — hard constraints (never violated) vs. soft constraints
  (weighted preference), aggregated into a priority-based score

The problem itself is modeled as a **constraint graph** (a hypergraph of
variables and constraints), not a tree — the tree only emerges as part of
a solver's search process.

## Relationship to `pathwise`

`pathwise` provides the generic `Problem` trait and interchangeable
search/optimization strategies (A*, branch and bound, local search,
simulated annealing, ...). `unifier` builds the CSP/COP-specific layer on
top: domain representation, the constraint graph, global constraints,
and a solver engine that reuses `pathwise`'s strategies where they fit
(e.g. branch and bound) and adds CSP-specific ones (constraint
propagation, arc consistency, MRV/fail-first variable ordering).

See `plan/01-concept.md` for the full architecture (4 layers: DSL,
constraint model, solver engine, runtime) and the MVP scope for 0.1.

## Installation

Not yet published to crates.io.

## License

MIT — see [LICENSE](LICENSE).
