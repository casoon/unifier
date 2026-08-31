//! Property-based testing for unifier propagation engine and solvers.
//!
//! Every property below asserts a concrete outcome for *every* generated input, rather than
//! only checking inside `if let SolveResult::Feasible { .. }` — a property that silently passes
//! on `Infeasible`/`Aborted` proves nothing (see `plan/08-project-evaluation.md`, finding #5).
//! Each problem here is constructed so its satisfiability is known analytically from the
//! generator's ranges, so the expected `SolveResult` variant is never in doubt.

use proptest::prelude::*;
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, LocalSearchSolver, SolveResult, SolverOptions};

proptest! {
    #[test]
    fn prop_equality_and_inequality_consistency(val_min in 1i64..5, val_max in 6i64..10) {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", val_min..=val_max);
        let y = builder.new_var("y", val_min..=val_max);

        // x < y  => x <= y - 1. `val_max >= 6 > 5 > val_min`, so the domain always has at least
        // two values and x = val_min, y = val_max is always a witness: always feasible.
        builder.add_less_than_or_equal(x, y, -1);

        let graph = builder.build().unwrap();
        let solver = BacktrackingSolver::new();
        let result = solver.solve(&graph, &SolverOptions::default());

        match result {
            SolveResult::Feasible { assignment, score, .. } => {
                prop_assert!(score.is_feasible());
                let vx = assignment[&x];
                let vy = assignment[&y];
                prop_assert!(vx < vy);
            }
            other => prop_assert!(false, "expected a feasible solution, got {other:?}"),
        }
    }

    #[test]
    fn prop_local_search_finds_feasible_equality(a in 1i64..10, b in 1i64..10) {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", 1..=20);
        let y = builder.new_var("y", 1..=20);

        // x = y + (a - b) with |a - b| <= 8 and both domains [1, 20]: always satisfiable, e.g.
        // y = 1, x = 1 + (a - b) when a >= b (and symmetrically otherwise).
        builder.add_equal(x, y, a - b);

        let graph = builder.build().unwrap();
        let solver = LocalSearchSolver::new(10);
        let result = solver.solve(&graph, &SolverOptions::default());

        match result {
            SolveResult::Feasible { assignment, score, .. } => {
                prop_assert!(score.is_feasible());
                let vx = assignment[&x];
                let vy = assignment[&y];
                prop_assert_eq!(vx, vy + (a - b));
            }
            other => prop_assert!(false, "expected a feasible solution, got {other:?}"),
        }
    }

    #[test]
    fn prop_exactly_one_cardinality(target_val in 1i64..5) {
        let mut builder = ModelBuilder::new();
        let vars: Vec<_> = (0..4).map(|i| builder.new_var(format!("v{}", i), 1..=5)).collect();

        // Domain [1, 5] has room for one variable at `target_val` and three at a different
        // value: always satisfiable.
        builder.add_exactly_one(vars.clone(), target_val);

        let graph = builder.build().unwrap();
        let solver = BacktrackingSolver::new();
        let result = solver.solve(&graph, &SolverOptions::default());

        match result {
            SolveResult::Feasible { assignment, score, .. } => {
                prop_assert!(score.is_feasible());
                let count = vars.iter().filter(|v| assignment.get(v) == Some(&target_val)).count();
                prop_assert_eq!(count, 1);
            }
            other => prop_assert!(false, "expected a feasible solution, got {other:?}"),
        }
    }

    #[test]
    fn prop_singleton_domain_not_equal_is_always_infeasible(v in 1i64..100) {
        // x and y are pinned to the very same singleton domain: x != y can never hold, for any
        // generated `v`. A dedicated "always unsatisfiable" property, exercising the opposite
        // branch of `SolveResult` from the properties above.
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", v..=v);
        let y = builder.new_var("y", v..=v);
        builder.add_not_equal(x, y);

        let graph = builder.build().unwrap();
        let result = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        prop_assert_eq!(result, SolveResult::Infeasible);
    }
}
