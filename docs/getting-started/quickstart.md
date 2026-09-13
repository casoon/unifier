---
title: Quickstart
description: Model a small optimization problem, solve it, and read the outcome.
order: 2
---

Three meetings have to fit into four time slots. No two may share a slot, meeting `a` has to
come before meeting `b`, and everything should happen as early as possible.

## 1. Build the model

```rust
use unifier::dsl::ModelBuilder;
use unifier::solver::{BranchAndBoundSolver, SolverOptions};

fn main() {
    let mut model = ModelBuilder::new();

    // Three meetings, each in one of the time slots 1 to 4.
    let a = model.new_var("a", 1..=4);
    let b = model.new_var("b", 1..=4);
    let c = model.new_var("c", 1..=4);

    // Hard constraints: no two meetings share a slot, and a ends before b starts.
    model.add_all_different([a, b, c]);
    model.add_less_than_or_equal(a, b, -1); // a <= b - 1

    // Soft objective: schedule everything as early as possible.
    model.add_minimize([a, b, c], 1);

    let graph = model.build().expect("valid model");
    let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());

    println!("status: {:?}", outcome.status);
    if let Some(solution) = outcome.solution {
        println!("score:  {}", solution.score);
        println!(
            "a = {}, b = {}, c = {}",
            solution.assignment[&a], solution.assignment[&b], solution.assignment[&c]
        );
    }
}
```

- `new_var` creates an integer variable with a range domain.
- `add_all_different` and `add_less_than_or_equal` are hard constraints: a solution that breaks
  them is not feasible.
- `add_minimize` adds a soft objective. Solvers maximize the soft score, so minimizing adds the
  sum with a negative weight.
- `build()` validates the model and returns the `ValidatedGraph` the solvers accept, or a list
  of `ModelError`s.

## 2. Run it

```text
status: Optimal
score:  Feasible(-6)
a = 1, b = 3, c = 2
```

`Optimal` means Branch & Bound searched the whole space and proved that no better score exists.
`Feasible(-6)` is the score: all hard constraints hold, and the minimized sum is 6. Slots 1, 2
and 3 are the earliest possible, but they can be distributed in several ways that satisfy
`a < b`; which of these equally good assignments you get can differ between runs.

## 3. Try another solver

All solvers share the same `solve(&graph, &options)` signature. Replace the solver line to
compare:

```rust
use unifier::solver::BacktrackingSolver;

let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
```

Backtracking stops at the first feasible assignment and reports `Feasible`, because it does not
try to prove optimality. The [solvers guide](../../guides/solvers/) explains what each strategy
can prove and how to set time limits.

## Next steps

- [Modelling](../../guides/modelling/): all constraints, objectives and the scheduling
  primitives.
- [Showcase](../../../showcase/): complete example programs with their output.
