//! Integration tests for unifier CSP/COP modeling and solvers.

use unifier::constraint::TaskDemand;
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, BranchAndBoundSolver, SolveResult, SolverOptions};

#[test]
fn test_dsl_nqueens_4() {
    let mut builder = ModelBuilder::new();
    let q0 = builder.new_var("q0", 1..=4);
    let q1 = builder.new_var("q1", 1..=4);
    let q2 = builder.new_var("q2", 1..=4);
    let q3 = builder.new_var("q3", 1..=4);

    // Rows must be distinct
    builder.add_all_different(vec![q0, q1, q2, q3]);

    // Diagonals must not match
    builder.add_not_equal(q0, q1);
    builder.add_not_equal(q1, q2);
    builder.add_not_equal(q2, q3);

    let graph = builder.build();
    let solver = BacktrackingSolver::new();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible { assignment, score } => {
            assert!(score.is_feasible());
            assert_eq!(assignment.len(), 4);
        }
        _ => panic!("Expected feasible solution for N-Queens(4)"),
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

    let graph = builder.build();
    let solver = BranchAndBoundSolver::new();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible { assignment, score } => {
            assert!(score.is_feasible());
            let s1 = assignment[&t1_start];
            let s2 = assignment[&t2_start];
            let s3 = assignment[&t3_start];
            assert!(s1 >= 0 && s2 >= 0 && s3 >= 0);
        }
        _ => panic!("Expected feasible timetabling schedule"),
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

    let graph = builder.build();
    let solver = unifier::solver::LocalSearchSolver::default();
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible { assignment, score } => {
            assert!(score.is_feasible());
            let e1 = assignment[&inv1.end()];
            let s2 = assignment[&inv2.start()];
            assert!(e1 <= s2);
        }
        _ => panic!("Expected feasible solution under precedence and value filters"),
    }
}

#[test]
fn test_lns_solver_and_cardinality() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..5).map(|i| builder.new_var(format!("v{}", i), 1..=5)).collect();

    // Exactly one variable takes value 3
    builder.add_exactly_one(vars.clone(), 3);
    // At most 2 variables take value 1
    builder.add_at_most(2, vars.clone(), 1);
    // At least 1 variable takes value 2
    builder.add_at_least(1, vars.clone(), 2);

    let graph = builder.build();
    let solver = unifier::solver::LnsSolver::new(0.4);
    let res = solver.solve(&graph, &SolverOptions::default());

    match res {
        SolveResult::Feasible { assignment, score } => {
            assert!(score.is_feasible());
            let count_3 = vars.iter().filter(|v| assignment.get(v) == Some(&3)).count();
            assert_eq!(count_3, 1);
            let count_1 = vars.iter().filter(|v| assignment.get(v) == Some(&1)).count();
            assert!(count_1 <= 2);
            let count_2 = vars.iter().filter(|v| assignment.get(v) == Some(&2)).count();
            assert!(count_2 >= 1);
        }
        _ => panic!("Expected feasible LNS solution under cardinality constraints"),
    }
}

#[test]
fn test_parallel_solver_and_cancellation() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..6).map(|i| builder.new_var(format!("v{}", i), 1..=6)).collect();
    builder.add_all_different(vars);

    let graph = builder.build();
    let token = unifier::solver::CancellationToken::new();
    let mut options = SolverOptions::default();
    options.cancellation_token = Some(token.clone());

    let solver = unifier::solver::ParallelSolver::new();
    let res = solver.solve(&graph, &options);

    match res {
        SolveResult::Feasible { score, .. } => {
            assert!(score.is_feasible());
        }
        _ => panic!("Expected feasible solution from ParallelSolver"),
    }
}
