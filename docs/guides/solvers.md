---
title: Solvers
description: The five solver strategies, what each can prove, and how to limit or cancel a run.
order: 2
---

Every solver takes a `ValidatedGraph` and `SolverOptions` and returns a `SolveOutcome`. Because
they share this signature, you can swap strategies without touching the model.

```rust
let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
```

## Strategies

| Solver | Kind | Can prove | Use it for |
| --- | --- | --- | --- |
| `BacktrackingSolver::new()` | complete tree search with propagation and `dom/wdeg` ordering | feasibility, infeasibility | pure CSP: any valid assignment, or proof that none exists |
| `BranchAndBoundSolver::new()` | complete tree search with propagation, MRV ordering and optimistic-bound pruning | optimality, infeasibility | COP when the optimum should be proven |
| `LocalSearchSolver::new(tabu_tenure)` | tabu search over value changes and swaps | – | a good solution quickly on large models |
| `LnsSolver::new(destroy_fraction)` | large neighbourhood search: relax part of the assignment, repair it by backtracking | – | improving an existing solution on large models |
| `ParallelSolver::new()` | portfolio of the four above, one thread each | what its workers prove | not knowing in advance which strategy fits |

`LocalSearchSolver` and `LnsSolver` also implement `Default`. `LnsSolver` clamps the destroy
fraction to the range 0.1 to 0.9.

`LnsSolver::solve_from(&graph, &baseline, &options)` starts from an existing assignment instead
of a fresh backtracking solution, for example an earlier plan after a constraint has changed.
The baseline is the centre of the neighbourhoods even when it is no longer feasible; only
feasible solutions are returned. If the baseline leaves a variable unassigned or holds a value
outside its domain, `solve_from` behaves like `solve`.

`ParallelSolver` runs Backtracking, Local Search, LNS and Branch & Bound at the same time. They
share a best-so-far solution (the shared incumbent), so a fast incomplete worker hands Branch &
Bound a strong bound to prune against. The result is the best solution any worker found, not the
first one reported, and every worker thread is joined before `solve` returns.

## Propagation

The complete solvers propagate after every decision: each constraint removes values from the
domains of its variables that can no longer be part of a solution, until nothing changes or a
domain runs empty. The engine is AC-3 with event-based re-queueing: only constraints on a
variable whose domain actually shrank run again. Domains are reversible (checkpoint and undo)
instead of being copied at every search node.

## Outcomes

`SolveOutcome` reports what the run established, separately from whether it found a solution:

| `SolveStatus` | Meaning | `solution` |
| --- | --- | --- |
| `Optimal` | a feasible solution, proven to have the best score; only Branch & Bound proves this, alone or inside the portfolio | present |
| `Feasible` | a feasible solution, not proven optimal | present |
| `Infeasible` | the search space was exhausted without a feasible assignment | absent |
| `Aborted(reason)` | the run stopped before it found a solution or proved anything | absent |

`Aborted` carries an `AbortReason`: `Cancelled`, `Timeout`, `NodeLimit`, or `LocalOptimum` when
local search has no improving, non-tabu move left. None of these prove infeasibility.

The outcome also includes `statistics` (nodes expanded, elapsed time) and, for Branch & Bound,
`bound`: an upper bound on the achievable soft score. When a search is stopped early, the gap
between the solution's score and `bound` shows how far from proven the result is.

When several solutions share the best score, which one is returned can differ between runs.

## Limits and cancellation

```rust
use std::time::Duration;
use unifier::CancellationToken;

let token = CancellationToken::new();
let options = SolverOptions {
    time_limit: Some(Duration::from_secs(2)),
    max_nodes: Some(1_000_000),
    cancellation_token: Some(token.clone()),
    ..SolverOptions::default()
};
// Another thread can call token.cancel() to stop the search.
```

| Field | Default | Effect |
| --- | --- | --- |
| `time_limit` | 10 seconds | stop after this duration |
| `max_nodes` | none | stop after expanding this many search nodes |
| `cancellation_token` | none | stop when the token is cancelled |
| `shared_incumbent` | none | set by `ParallelSolver` for its workers; leave it empty |

A run that stops after finding a solution returns `Feasible` with the best solution so far,
which is what makes the solvers usable as anytime solvers. `ParallelSolver` only observes the
token you pass in and never cancels it itself, so the token can be reused.
