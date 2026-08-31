//! Property-based testing for unifier propagation engine and solvers.
//!
//! Every property below asserts a concrete outcome for *every* generated input, rather than
//! only checking inside a `Some(solution)` branch — a property that silently passes when no
//! solution is found proves nothing (see `plan/08-project-evaluation.md`, finding #5). Each
//! problem here is constructed so its satisfiability is known analytically from the generator's
//! ranges, so the expected outcome is never in doubt.
//!
//! The `prop_branch_and_bound_*_matches_oracle` properties additionally differential-test
//! `BranchAndBoundSolver` against a brute-force oracle across randomized small COP models built
//! from `ExactlyOne`/`AtLeast` + a `WeightedSum` objective — the exact constraint/objective
//! combination that produced a false optimality proof before
//! `plan/09-project-reevaluation-roadmap.md`'s P0 fix (see
//! `src/solver/branch_and_bound.rs`'s unit test and `tests/integration_test.rs`'s
//! `test_exactly_one_partial_cardinality_optimality_matches_oracle` for the specific
//! minimal-reproduction case).

use proptest::prelude::*;
use std::collections::HashMap;
use unifier::dsl::ModelBuilder;
use unifier::score::{HardSoftScore, ScoreCalculator};
use unifier::solver::{
    BacktrackingSolver, BranchAndBoundSolver, LocalSearchSolver, SolveStatus, SolverOptions,
};
use unifier::{ConstraintGraph, VariableId};

/// Exhaustively enumerates every complete assignment of `graph`'s (small!) domains and returns
/// the best `HardSoftScore` among assignments satisfying every constraint, or `None` if none do.
/// See `tests/integration_test.rs` for the identical helper used there.
fn brute_force_best(graph: &ConstraintGraph) -> Option<HardSoftScore> {
    let vars: Vec<VariableId> = graph.variables().keys().copied().collect();
    let domains: Vec<Vec<i64>> = vars.iter().map(|v| graph.domains()[v].values()).collect();
    let calculator = ScoreCalculator;
    let mut best: Option<HardSoftScore> = None;
    let mut current = HashMap::new();

    fn recurse(
        idx: usize,
        vars: &[VariableId],
        domains: &[Vec<i64>],
        current: &mut HashMap<VariableId, i64>,
        graph: &ConstraintGraph,
        calculator: &ScoreCalculator,
        best: &mut Option<HardSoftScore>,
    ) {
        if idx == vars.len() {
            if graph.constraints().iter().all(|c| c.is_satisfied(current)) {
                let score = calculator.calculate_score(graph, current);
                if best.is_none_or(|b| score > b) {
                    *best = Some(score);
                }
            }
            return;
        }
        for &val in &domains[idx] {
            current.insert(vars[idx], val);
            recurse(idx + 1, vars, domains, current, graph, calculator, best);
        }
        current.remove(&vars[idx]);
    }

    recurse(
        0,
        &vars,
        &domains,
        &mut current,
        graph,
        &calculator,
        &mut best,
    );
    best
}

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
        let outcome = solver.solve(&graph, &SolverOptions::default());

        match outcome.solution {
            Some(solution) => {
                prop_assert!(solution.score.is_feasible());
                let vx = solution.assignment[&x];
                let vy = solution.assignment[&y];
                prop_assert!(vx < vy);
            }
            None => prop_assert!(false, "expected a feasible solution, got status {:?}", outcome.status),
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
        let outcome = solver.solve(&graph, &SolverOptions::default());

        match outcome.solution {
            Some(solution) => {
                prop_assert!(solution.score.is_feasible());
                let vx = solution.assignment[&x];
                let vy = solution.assignment[&y];
                prop_assert_eq!(vx, vy + (a - b));
            }
            None => prop_assert!(false, "expected a feasible solution, got status {:?}", outcome.status),
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
        let outcome = solver.solve(&graph, &SolverOptions::default());

        match outcome.solution {
            Some(solution) => {
                prop_assert!(solution.score.is_feasible());
                let count = vars.iter().filter(|v| solution.assignment.get(v) == Some(&target_val)).count();
                prop_assert_eq!(count, 1);
            }
            None => prop_assert!(false, "expected a feasible solution, got status {:?}", outcome.status),
        }
    }

    #[test]
    fn prop_singleton_domain_not_equal_is_always_infeasible(v in 1i64..100) {
        // x and y are pinned to the very same singleton domain: x != y can never hold, for any
        // generated `v`. A dedicated "always unsatisfiable" property, exercising the opposite
        // outcome from the properties above.
        let mut builder = ModelBuilder::new();
        let x = builder.new_var("x", v..=v);
        let y = builder.new_var("y", v..=v);
        builder.add_not_equal(x, y);

        let graph = builder.build().unwrap();
        let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
        prop_assert_eq!(outcome.status, SolveStatus::Infeasible);
        prop_assert!(outcome.solution.is_none());
    }

    #[test]
    fn prop_branch_and_bound_exactly_one_matches_oracle(target in 0i64..3, weight in -3i64..=3) {
        // 3 variables in a small domain, ExactlyOne(target) + a weighted objective on one
        // variable: this is exactly the constraint/objective shape that produced a false
        // optimality proof before the P0 fix (see module doc comment).
        let mut builder = ModelBuilder::new();
        let vars: Vec<_> = (0..3).map(|i| builder.new_var(format!("v{i}"), 0i64..=2)).collect();
        builder.add_exactly_one(vars.clone(), target);
        if weight >= 0 {
            builder.add_maximize(vec![vars[0]], weight);
        } else {
            builder.add_minimize(vec![vars[0]], -weight);
        }
        let graph = builder.build().unwrap();

        let oracle = brute_force_best(&graph);
        let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());

        match (oracle, outcome.status, outcome.solution) {
            (Some(oracle_score), SolveStatus::Optimal, Some(solution)) => {
                prop_assert_eq!(solution.score, oracle_score);
            }
            (None, SolveStatus::Infeasible, None) => {}
            (oracle, status, solution) => prop_assert!(
                false,
                "oracle={oracle:?} disagrees with solver status={status:?} solution={solution:?}"
            ),
        }
    }

    #[test]
    fn prop_branch_and_bound_at_least_matches_oracle(target in 0i64..3, k in 1usize..=3, weight in -3i64..=3) {
        let mut builder = ModelBuilder::new();
        let vars: Vec<_> = (0..3).map(|i| builder.new_var(format!("v{i}"), 0i64..=2)).collect();
        builder.add_at_least(k, vars.clone(), target);
        if weight >= 0 {
            builder.add_maximize(vec![vars[0]], weight);
        } else {
            builder.add_minimize(vec![vars[0]], -weight);
        }
        let graph = builder.build().unwrap();

        let oracle = brute_force_best(&graph);
        let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());

        match (oracle, outcome.status, outcome.solution) {
            (Some(oracle_score), SolveStatus::Optimal, Some(solution)) => {
                prop_assert_eq!(solution.score, oracle_score);
            }
            (None, SolveStatus::Infeasible, None) => {}
            (oracle, status, solution) => prop_assert!(
                false,
                "oracle={oracle:?} disagrees with solver status={status:?} solution={solution:?}"
            ),
        }
    }

    #[test]
    fn prop_branch_and_bound_is_deterministic_across_runs(target in 0i64..3, weight in 1i64..=3) {
        // Same model, same (default, unseeded) options, run twice: the solver must not depend on
        // process-specific nondeterminism (e.g. HashMap iteration order) for *which* optimal
        // score it reports, even though which of several equally-optimal assignments it returns
        // is not guaranteed.
        let mut builder = ModelBuilder::new();
        let vars: Vec<_> = (0..3).map(|i| builder.new_var(format!("v{i}"), 0i64..=2)).collect();
        builder.add_exactly_one(vars.clone(), target);
        builder.add_maximize(vec![vars[0]], weight);
        let graph = builder.build().unwrap();

        let solver = BranchAndBoundSolver::new();
        let first = solver.solve(&graph, &SolverOptions::default());
        let second = solver.solve(&graph, &SolverOptions::default());

        prop_assert_eq!(first.status, second.status);
        prop_assert_eq!(first.solution.map(|s| s.score), second.solution.map(|s| s.score));
    }
}
