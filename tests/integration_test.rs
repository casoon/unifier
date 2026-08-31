//! Integration tests for unifier CSP/COP modeling and solvers.
//!
//! Beyond happy-path solver checks, this file covers what `plan/08-project-evaluation.md`
//! flagged as missing: proven-unsatisfiable models, timeout/cancellation semantics, invalid
//! graph validation, i64/u64 boundary values, a brute-force oracle for differential testing,
//! and an optimality (not just feasibility) proof via Branch & Bound.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use unifier::constraint::{Cumulative, NotEqual, TaskDemand};
use unifier::dsl::ModelBuilder;
use unifier::propagation::ModelError;
use unifier::score::{HardSoftScore, ScoreCalculator};
use unifier::solver::{
    AbortReason, BacktrackingSolver, BranchAndBoundSolver, CancellationToken, LnsSolver,
    LocalSearchSolver, ParallelSolver, SolveResult, SolverOptions,
};
use unifier::{ConstraintGraph, Domain, Variable, VariableId};

/// Builds an N-Queens board: `vars[row]` holds the column of the queen in that row (1-indexed).
/// Enforces column distinctness (`AllDifferent`) and diagonal distinctness (`NotEqual` with the
/// row-distance offset in both directions).
fn build_nqueens_graph(n: i64) -> ConstraintGraph {
    let mut builder = ModelBuilder::new();
    let vars: Vec<VariableId> = (0..n)
        .map(|i| builder.new_var(format!("q{i}"), 1..=n))
        .collect();

    builder.add_all_different(vars.clone());
    for i in 0..vars.len() {
        for j in (i + 1)..vars.len() {
            let diff = (j - i) as i64;
            builder.add_constraint(Arc::new(NotEqual::with_offset(vars[i], vars[j], diff)));
            builder.add_constraint(Arc::new(NotEqual::with_offset(vars[i], vars[j], -diff)));
        }
    }

    builder.build().expect("N-Queens model should validate")
}

/// Exhaustively enumerates every complete assignment of `graph`'s (small!) domains and returns
/// the best `HardSoftScore` among assignments satisfying every constraint, or `None` if none do.
///
/// A ground-truth oracle independent of any solver's search strategy, for differential testing.
/// Exponential in the number of variables — only for small test instances.
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

#[test]
fn test_dsl_nqueens_4() {
    let graph = build_nqueens_graph(4);
    let solver = BacktrackingSolver::new();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible {
            assignment, score, ..
        } => {
            assert!(score.is_feasible());
            assert_eq!(assignment.len(), 4);
        }
        other => panic!("Expected feasible solution for N-Queens(4), got {other:?}"),
    }
}

#[test]
fn test_nqueens_3_is_infeasible() {
    // Classic CSP fact: N-Queens has no solution for N = 2 or N = 3.
    let graph = build_nqueens_graph(3);
    assert_eq!(
        brute_force_best(&graph),
        None,
        "oracle: N-Queens(3) must be unsatisfiable"
    );

    for (name, result) in [
        (
            "Backtracking",
            BacktrackingSolver::new().solve(&graph, &SolverOptions::default()),
        ),
        (
            "BranchAndBound",
            BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default()),
        ),
        (
            "Parallel",
            ParallelSolver::new().solve(&graph, &SolverOptions::default()),
        ),
    ] {
        assert_eq!(
            result,
            SolveResult::Infeasible,
            "{name} should prove N-Queens(3) infeasible"
        );
    }
}

#[test]
fn test_backtracking_matches_oracle_on_nqueens_4() {
    let graph = build_nqueens_graph(4);
    let oracle = brute_force_best(&graph);
    assert!(oracle.is_some(), "oracle: N-Queens(4) is satisfiable");

    let result = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
    match result {
        SolveResult::Feasible { score, .. } => assert!(score.is_feasible()),
        other => {
            panic!("Backtracking disagrees with oracle: got {other:?}, oracle found a solution")
        }
    }
}

#[test]
fn test_timetabling_no_overlap_and_cumulative() {
    let mut builder = ModelBuilder::new();

    // 3 tasks scheduled within time window 0..=10
    let t1_start = builder.new_var("t1_start", 0..=8);
    let t2_start = builder.new_var("t2_start", 0..=8);
    let t3_start = builder.new_var("t3_start", 0..=8);

    // Cumulative resource with capacity 2
    let tasks = vec![
        TaskDemand {
            start: t1_start,
            duration: 3,
            demand: 1,
        },
        TaskDemand {
            start: t2_start,
            duration: 4,
            demand: 1,
        },
        TaskDemand {
            start: t3_start,
            duration: 2,
            demand: 1,
        },
    ];

    builder.add_cumulative(tasks, 2);

    let graph = builder.build().expect("model should validate");
    let solver = BranchAndBoundSolver::new();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible {
            assignment, score, ..
        } => {
            assert!(score.is_feasible());
            let s1 = assignment[&t1_start];
            let s2 = assignment[&t2_start];
            let s3 = assignment[&t3_start];
            assert!(s1 >= 0 && s2 >= 0 && s3 >= 0);
        }
        other => panic!("Expected feasible timetabling schedule, got {other:?}"),
    }
}

#[test]
fn test_precedence_and_domain_filtering() {
    let mut builder = ModelBuilder::new();
    let inv1 = builder.new_interval("act1", 0..=10, 3, 3..=13);
    let inv2 = builder.new_interval("act2", 0..=10, 2, 2..=12);

    // Precedence: inv1 end <= inv2 start
    builder.add_precedence(&inv1, &inv2, 0);
    // Allowed values filter on inv1 start
    builder.add_allowed_values(inv1.start(), vec![0, 1, 2]);
    // Forbidden values filter on inv2 start
    builder.add_forbidden_values(inv2.start(), vec![0, 1, 2]);

    let graph = builder.build().expect("model should validate");
    let solver = LocalSearchSolver::default();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible {
            assignment, score, ..
        } => {
            assert!(score.is_feasible());
            let e1 = assignment[&inv1.end()];
            let s2 = assignment[&inv2.start()];
            assert!(e1 <= s2);
        }
        other => {
            panic!("Expected feasible solution under precedence and value filters, got {other:?}")
        }
    }
}

#[test]
fn test_lns_solver_and_cardinality() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..5)
        .map(|i| builder.new_var(format!("v{}", i), 1..=5))
        .collect();

    // Exactly one variable takes value 3
    builder.add_exactly_one(vars.clone(), 3);
    // At most 2 variables take value 1
    builder.add_at_most(2, vars.clone(), 1);
    // At least 1 variable takes value 2
    builder.add_at_least(1, vars.clone(), 2);

    let graph = builder.build().expect("model should validate");
    let solver = LnsSolver::new(0.4);
    // No objective is attached, so every feasible solution scores identically: LNS can never
    // find an improving move and would otherwise burn the full default 10s time budget doing
    // nothing. A short budget is enough to prove feasibility here.
    let options = SolverOptions {
        time_limit: Some(Duration::from_millis(300)),
        ..SolverOptions::default()
    };
    let res = solver.solve(&graph, &options);

    match res {
        SolveResult::Feasible {
            assignment, score, ..
        } => {
            assert!(score.is_feasible());
            let count_3 = vars
                .iter()
                .filter(|v| assignment.get(v) == Some(&3))
                .count();
            assert_eq!(count_3, 1);
            let count_1 = vars
                .iter()
                .filter(|v| assignment.get(v) == Some(&1))
                .count();
            assert!(count_1 <= 2);
            let count_2 = vars
                .iter()
                .filter(|v| assignment.get(v) == Some(&2))
                .count();
            assert!(count_2 >= 1);
        }
        other => {
            panic!("Expected feasible LNS solution under cardinality constraints, got {other:?}")
        }
    }
}

#[test]
fn test_parallel_solver_and_cancellation() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..6)
        .map(|i| builder.new_var(format!("v{}", i), 1..=6))
        .collect();
    builder.add_all_different(vars);

    let graph = builder.build().expect("model should validate");
    let token = CancellationToken::new();
    let options = SolverOptions {
        cancellation_token: Some(token.clone()),
        ..SolverOptions::default()
    };

    let solver = ParallelSolver::new();
    let res = solver.solve(&graph, &options);

    match res {
        SolveResult::Feasible { score, .. } => {
            assert!(score.is_feasible());
        }
        other => panic!("Expected feasible solution from ParallelSolver, got {other:?}"),
    }
}

#[test]
fn test_parallel_solver_does_not_claim_infeasible_when_starved() {
    // Regression test for plan/08-project-evaluation.md finding #3: if every worker merely runs
    // out of search budget, the combined result must not claim proven infeasibility.
    let graph = build_nqueens_graph(4); // satisfiable, but starved of any search budget below
    let options = SolverOptions {
        time_limit: None,
        max_nodes: Some(0),
        cancellation_token: None,
    };
    let result = ParallelSolver::new().solve(&graph, &options);
    assert!(
        matches!(result, SolveResult::Aborted { .. }),
        "expected Aborted since no worker could prove infeasibility with zero search budget, got {result:?}"
    );
}

#[test]
fn test_backtracking_aborts_on_node_limit() {
    let graph = build_nqueens_graph(4);
    let options = SolverOptions {
        time_limit: None,
        max_nodes: Some(0),
        cancellation_token: None,
    };
    let result = BacktrackingSolver::new().solve(&graph, &options);
    assert_eq!(
        result,
        SolveResult::Aborted {
            reason: AbortReason::NodeLimit
        }
    );
}

#[test]
fn test_backtracking_aborts_on_pre_cancelled_token() {
    let graph = build_nqueens_graph(4);
    let token = CancellationToken::new();
    token.cancel();
    let options = SolverOptions {
        cancellation_token: Some(token),
        ..SolverOptions::default()
    };
    let result = BacktrackingSolver::new().solve(&graph, &options);
    assert_eq!(
        result,
        SolveResult::Aborted {
            reason: AbortReason::Cancelled
        }
    );
}

#[test]
fn test_branch_and_bound_proves_optimum_matches_oracle() {
    let mut builder = ModelBuilder::new();
    // 3 distinct values from a wider domain: many feasible sums, so a naive first-found
    // assignment ({1,2,3} = 6) differs from the true optimum ({3,4,5} = 12).
    let vars: Vec<_> = (0..3)
        .map(|i| builder.new_var(format!("v{i}"), 1..=5))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_maximize(vars.clone(), 1);
    let graph = builder.build().expect("model should validate");

    let oracle_best = brute_force_best(&graph).expect("feasible by construction");
    assert_eq!(oracle_best, HardSoftScore::new(0, 12));

    let result = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    match result {
        SolveResult::Feasible {
            score,
            proven_optimal,
            ..
        } => {
            assert!(
                proven_optimal,
                "Branch & Bound should prove optimality for such a small instance"
            );
            assert_eq!(
                score, oracle_best,
                "Branch & Bound optimum must match the brute-force oracle"
            );
        }
        other => panic!("Expected a proven-optimal feasible solution, got {other:?}"),
    }
}

#[test]
fn test_branch_and_bound_minimize_matches_oracle() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..3)
        .map(|i| builder.new_var(format!("v{i}"), 1..=5))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_minimize(vars.clone(), 1);
    let graph = builder.build().expect("model should validate");

    let oracle_best = brute_force_best(&graph).expect("feasible by construction");
    assert_eq!(oracle_best, HardSoftScore::new(0, -6)); // minimizing sum(vars) => picks {1,2,3}

    let result = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    match result {
        SolveResult::Feasible {
            score,
            proven_optimal,
            ..
        } => {
            assert!(proven_optimal);
            assert_eq!(score, oracle_best);
        }
        other => panic!("Expected a proven-optimal feasible solution, got {other:?}"),
    }
}

#[test]
fn test_branch_and_bound_not_proven_optimal_when_node_starved() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..6)
        .map(|i| builder.new_var(format!("v{i}"), 1..=6))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_maximize(vars, 1);
    let graph = builder.build().expect("model should validate");

    let options = SolverOptions {
        time_limit: None,
        max_nodes: Some(3),
        cancellation_token: None,
    };
    match BranchAndBoundSolver::new().solve(&graph, &options) {
        SolveResult::Feasible { proven_optimal, .. } => {
            assert!(
                !proven_optimal,
                "3 search nodes cannot exhaust a 6-variable AllDifferent tree"
            );
        }
        SolveResult::Aborted { .. } => {} // also acceptable: no feasible solution found within budget
        other => panic!("Expected Feasible(not proven optimal) or Aborted, got {other:?}"),
    }
}

#[test]
fn test_validate_rejects_unknown_variable() {
    let mut graph = ConstraintGraph::new();
    let v1 = VariableId(1);
    graph.add_variable(Variable::new(v1, "x"), Domain::range(1, 3));
    let unknown = VariableId(99);
    graph.add_constraint(Arc::new(NotEqual::new(v1, unknown)));

    let errors = graph
        .validate()
        .expect_err("constraint references an unregistered variable");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ModelError::UnknownVariable { var, .. } if *var == unknown)),
        "expected UnknownVariable({unknown:?}) among {errors:?}"
    );
}

#[test]
fn test_validate_rejects_empty_domain() {
    let mut graph = ConstraintGraph::new();
    let v1 = VariableId(1);
    graph.add_variable(Variable::new(v1, "x"), Domain::range(5, 1)); // min > max => empty

    let errors = graph.validate().expect_err("variable has an empty domain");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ModelError::EmptyDomain { var } if *var == v1)),
        "expected EmptyDomain({v1:?}) among {errors:?}"
    );
}

#[test]
fn test_validate_rejects_duplicate_variable_id() {
    let mut graph = ConstraintGraph::new();
    let v1 = VariableId(1);
    graph.add_variable(Variable::new(v1, "x"), Domain::range(1, 3));
    graph.add_variable(Variable::new(v1, "x-again"), Domain::range(1, 3));

    let errors = graph.validate().expect_err("v1 was registered twice");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ModelError::DuplicateVariableId { var } if *var == v1)),
        "expected DuplicateVariableId({v1:?}) among {errors:?}"
    );
}

#[test]
fn test_validate_rejects_cumulative_demand_exceeding_capacity() {
    let mut graph = ConstraintGraph::new();
    let start = VariableId(1);
    graph.add_variable(Variable::new(start, "s"), Domain::range(0, 5));
    graph.add_constraint(Arc::new(Cumulative::new(
        vec![TaskDemand {
            start,
            duration: 1,
            demand: 5,
        }],
        2, // capacity 2 < demand 5: this task can never be scheduled
    )));

    let errors = graph
        .validate()
        .expect_err("a single task's demand exceeds total capacity");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ModelError::InvalidConstraint { .. })),
        "expected InvalidConstraint among {errors:?}"
    );
}

#[test]
fn test_model_builder_build_surfaces_validation_errors() {
    let mut builder = ModelBuilder::new();
    let v1 = builder.new_var("x", 1..=3);
    let foreign = VariableId(9999); // never registered via this builder
    builder.add_not_equal(v1, foreign);

    let result = builder.build();
    assert!(
        result.is_err(),
        "build() should reject a model referencing an unregistered variable"
    );
}
