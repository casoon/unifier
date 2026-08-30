//! Property-based testing for unifier propagation engine and solvers.

use proptest::prelude::*;
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, LocalSearchSolver, SolveResult, SolverOptions};

proptest! {
    #[test]
    fn prop_equality_and_inequality_consistency(val_min in 1i64..5, val_max in 6i64..10) {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", val_min..=val_max);
        let y = builder.new_var("y", val_min..=val_max);

        // x < y  => x <= y - 1
        builder.add_less_than_or_equal(x, y, -1);

        let graph = builder.build();
        let solver = BacktrackingSolver::new();
        let result = solver.solve(&graph, &SolverOptions::default());

        if let SolveResult::Feasible { assignment, score } = result {
            prop_assert!(score.is_feasible());
            let vx = assignment[&x];
            let vy = assignment[&y];
            prop_assert!(vx < vy);
        }
    }

    #[test]
    fn prop_local_search_finds_feasible_equality(a in 1i64..10, b in 1i64..10) {
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", 1..=20);
        let y = builder.new_var("y", 1..=20);

        builder.add_equal(x, y, a - b);

        let graph = builder.build();
        let solver = LocalSearchSolver::new(10);
        let result = solver.solve(&graph, &SolverOptions::default());

        if let SolveResult::Feasible { assignment, score } = result {
            prop_assert!(score.is_feasible());
            let vx = assignment[&x];
            let vy = assignment[&y];
            prop_assert_eq!(vx, vy + (a - b));
        }
    }

    #[test]
    fn prop_exactly_one_cardinality(target_val in 1i64..5) {
        let mut builder = ModelBuilder::new();
        let vars: Vec<_> = (0..4).map(|i| builder.new_var(format!("v{}", i), 1..=5)).collect();

        builder.add_exactly_one(vars.clone(), target_val);

        let graph = builder.build();
        let solver = BacktrackingSolver::new();
        let result = solver.solve(&graph, &SolverOptions::default());

        if let SolveResult::Feasible { assignment, score } = result {
            prop_assert!(score.is_feasible());
            let count = vars.iter().filter(|v| assignment.get(v) == Some(&target_val)).count();
            prop_assert_eq!(count, 1);
        }
    }
}
