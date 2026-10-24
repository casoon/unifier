# unifier

Rust crate: modelling and solver framework for constraint satisfaction / optimization problems
(CSP/COP), built on the generic search and optimization building blocks of
[`pathwise`](https://github.com/casoon/pathwise). Concept and background: `README.md`. Local
status and plans: `plan/` (gitignored).

Published on crates.io as `unifier`; docs at https://casoon.github.io/unifier/.

## Positioning

`pathwise` provides generic search and optimization strategies (A*, Branch & Bound, local search,
...) behind a `Problem` trait. `unifier` adds the CSP/COP layer on top: variables and domains, a
constraint graph, global constraints (`AllDifferent`, `NoOverlap`, `Cumulative`, ...), hard/soft
scoring, constraint propagation. It is not a stand-alone replacement for OR-Tools, MiniZinc or
Timefold — those are references for concepts (global constraints, score model), not a source of
code to copy.

## Architecture

```
DSL              — problem-building surface API (activity/resource/rule)
Constraint Model — variable, domain, constraint graph (hypergraph)
Solver Engine    — propagation, backtracking, Branch & Bound,
                    local search, LNS, parallel portfolio
Runtime          — incremental scoring, cancellation/timeout,
                    anytime solving
```

The problem structure is a **constraint graph**, not a tree — a tree only appears while a solver
works (the backtracking search tree, Branch & Bound branches).

`unifier` is the CSP/COP layer in the stack `pathwise → unifier → schedulr → applications`. The
scheduling primitives (`Activity`, `Interval`, `Resource`, `Group`) and their compiler stay in
`unifier` as the foundation; `schedulr` wraps them behind its domain-neutral API and adds state
and booking logic.

## Working

- Current status and next step: `plan/00-STATUS.md`.
- Before a push: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
  `cargo doc --no-deps` without warnings.

## Fixed rules

- Language: **English** for everything in the repository — code, identifiers, comments, doc
  comments, string literals, tests, docs and commit messages. Only `plan/` (local, gitignored)
  may be German.
- License: **MIT** (`Cargo.toml`: `license = "MIT"`).
- No `unsafe` without an explicit reason and a comment.
- Do not implement what `pathwise`, `std` or an established crate already solves well.
- Every public function and every constraint documents complexity, preconditions and a reference
  (original paper or textbook), not just a one-line summary.
