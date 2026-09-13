---
title: Relation to pathwise
description: What unifier takes from pathwise, and what it deliberately does itself.
order: 3
---

[pathwise](https://github.com/casoon/pathwise) is a separate crate with generic search and
optimization building blocks: a `Problem` trait (state, moves, cost or score) and strategies
that run against it interchangeably, such as A\*, branch and bound, local search and simulated
annealing. unifier depends on it, but uses much less of it than the name "built on pathwise"
might suggest.

## What unifier uses

Two CSP-independent coordination primitives:

| In pathwise | In unifier | Purpose |
| --- | --- | --- |
| `pathwise::core::cancellation::CancellationToken` | re-exported as `unifier::CancellationToken` | A cloneable flag to stop a running search from another thread. |
| `pathwise::core::incumbent::SharedIncumbent` | wrapped by `unifier::solver::SharedIncumbent` | The best solution found so far, shared between portfolio workers. |

`SharedIncumbent` in unifier is a thin wrapper that specializes the generic type to unifier's
assignment (`HashMap<VariableId, i64>`) and score (`HardSoftScore`). Using both from pathwise
avoids keeping a second copy of the same logic.

## What unifier does itself

unifier does not implement pathwise's `Problem`/`OptimizationProblem` traits and does not route
its search through pathwise's generic algorithms. Its solvers are specialized for constraint
models:

- constraint propagation, with generalized arc consistency for `AllDifferent` (Régin's
  matching and SCC algorithm), overload detection for `Cumulative` and `NoOverlap`,
  edge-finding for `NoOverlap`, and AC-3 otherwise;
- `dom/wdeg` and MRV variable ordering;
- reversible domains (checkpoint and undo) instead of cloning the domains at every search node;
- Branch & Bound with its own optimistic-bound pruning over hard/soft scores.

A generic state-space search cannot see domains or constraints, so it cannot prune with
propagation. That is why this layer lives in unifier rather than in pathwise.

## In practice

You don't need to use pathwise directly. `unifier::CancellationToken` is the type you pass in
[`SolverOptions`](../solvers/#limits-and-cancellation), and `SharedIncumbent` is set up for you by
`ParallelSolver`.
