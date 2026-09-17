//! Integration tests for unifier CSP/COP modeling and solvers.
//!
//! Beyond happy-path solver checks, this file covers what
//! `plan/08-project-evaluation.md` and `plan/09-project-reevaluation-roadmap.md` flagged as
//! missing: proven-unsatisfiable models, timeout/cancellation semantics, invalid graph
//! validation, i64/u64 boundary values, a brute-force oracle for differential testing, and
//! optimality (not just feasibility) proofs via Branch & Bound — including the specific
//! partial-cardinality-constraint case that previously produced a false optimality proof.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use unifier::constraint::{Constraint, Cumulative, NotEqual, TaskDemand};
use unifier::dsl::ModelBuilder;
use unifier::propagation::ModelError;
use unifier::score::{HardSoftScore, ScoreCalculator};
use unifier::solver::{
    AbortReason, BacktrackingSolver, BranchAndBoundSolver, CancellationToken, LnsSolver,
    LocalSearchSolver, ParallelSolver, SolveStatus, SolverOptions,
};
use unifier::{ConstraintGraph, Domain, ValidatedGraph, Variable, VariableId};

/// Builds an N-Queens board: `vars[row]` holds the column of the queen in that row (1-indexed).
/// Enforces column distinctness (`AllDifferent`) and diagonal distinctness (`NotEqual` with the
/// row-distance offset in both directions).
fn build_nqueens_graph(n: i64) -> ValidatedGraph {
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
    let outcome = solver.solve(&graph, &SolverOptions::default());

    match outcome.solution {
        Some(solution) => {
            assert!(solution.score.is_feasible());
            assert_eq!(solution.assignment.len(), 4);
        }
        None => panic!(
            "Expected feasible solution for N-Queens(4), got status {:?}",
            outcome.status
        ),
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

    for (name, outcome) in [
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
            outcome.status,
            SolveStatus::Infeasible,
            "{name} should prove N-Queens(3) infeasible"
        );
        assert!(outcome.solution.is_none());
    }
}

#[test]
fn test_backtracking_matches_oracle_on_nqueens_4() {
    let graph = build_nqueens_graph(4);
    let oracle = brute_force_best(&graph);
    assert!(oracle.is_some(), "oracle: N-Queens(4) is satisfiable");

    let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
    match outcome.solution {
        Some(solution) => assert!(solution.score.is_feasible()),
        None => panic!(
            "Backtracking disagrees with oracle: got status {:?}, oracle found a solution",
            outcome.status
        ),
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
    let outcome = solver.solve(&graph, &SolverOptions::default());

    match outcome.solution {
        Some(solution) => {
            assert!(solution.score.is_feasible());
            let s1 = solution.assignment[&t1_start];
            let s2 = solution.assignment[&t2_start];
            let s3 = solution.assignment[&t3_start];
            assert!(s1 >= 0 && s2 >= 0 && s3 >= 0);
        }
        None => panic!(
            "Expected feasible timetabling schedule, got status {:?}",
            outcome.status
        ),
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
    let outcome = solver.solve(&graph, &SolverOptions::default());

    match outcome.solution {
        Some(solution) => {
            assert!(solution.score.is_feasible());
            let e1 = solution.assignment[&inv1.end()];
            let s2 = solution.assignment[&inv2.start()];
            assert!(e1 <= s2);
        }
        None => panic!(
            "Expected feasible solution under precedence and value filters, got status {:?}",
            outcome.status
        ),
    }
}

#[test]
fn test_incremental_check_matches_full_feasibility_without_search() {
    let mut builder = ModelBuilder::new();
    let first = builder.new_interval("first", 0..=0, 3, 3..=3);
    let second = builder.new_interval("second", 0..=4, 3, 3..=7);
    builder.add_no_overlap(&[first.clone(), second.clone()], &[3, 3]);
    let graph = builder.build().expect("model should validate");

    let committed = HashMap::from([(first.start(), 0), (first.end(), 3)]);
    let violations = graph.check_incremental(&committed, &[(second.start(), 1), (second.end(), 4)]);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].constraint_name, "NoOverlap");
    assert_eq!(violations[0].involved, vec![first.start(), second.start()]);

    let mut fixed_builder = ModelBuilder::new();
    let fixed_first = fixed_builder.new_interval("first", 0..=0, 3, 3..=3);
    let fixed_second = fixed_builder.new_interval("second", 1..=1, 3, 4..=4);
    fixed_builder.add_no_overlap(&[fixed_first, fixed_second], &[3, 3]);
    let fixed_graph = fixed_builder.build().expect("model should validate");
    let outcome = BacktrackingSolver::new().solve(&fixed_graph, &SolverOptions::default());
    assert_eq!(outcome.status, SolveStatus::Infeasible);
    assert_eq!(outcome.statistics.nodes_expanded, 0);
}

#[test]
fn test_structured_explanations_for_scheduling_constraints() {
    use unifier::constraint::no_overlap::{NoOverlap, TaskInterval};
    use unifier::model::interval::DurationSpec;
    use unifier::{Interval, Precedence};

    let a = VariableId(10);
    let b = VariableId(11);
    let no_overlap = NoOverlap::new(vec![
        TaskInterval {
            start: a,
            duration: 3,
        },
        TaskInterval {
            start: b,
            duration: 3,
        },
    ]);
    let assignment = HashMap::from([(a, 0), (b, 2)]);
    let explanation = no_overlap.explain(&assignment).expect("overlap explained");
    assert_eq!(explanation.constraint_name, "NoOverlap");
    assert_eq!(explanation.involved, vec![a, b]);

    let cumulative = Cumulative::new(
        vec![
            TaskDemand {
                start: a,
                duration: 3,
                demand: 2,
            },
            TaskDemand {
                start: b,
                duration: 3,
                demand: 1,
            },
        ],
        2,
    );
    let explanation = cumulative
        .explain(&assignment)
        .expect("capacity overload explained");
    assert_eq!(explanation.constraint_name, "Cumulative");
    assert_eq!(explanation.involved, vec![a, b]);

    let predecessor = Interval::new(a, DurationSpec::Fixed(1), VariableId(12));
    let successor = Interval::new(b, DurationSpec::Fixed(1), VariableId(13));
    let precedence = Precedence::new(&predecessor, &successor, 1);
    let assignment = HashMap::from([(VariableId(12), 5), (b, 5)]);
    let explanation = precedence
        .explain(&assignment)
        .expect("precedence violation explained");
    assert_eq!(explanation.constraint_name, "Precedence");
    assert_eq!(explanation.involved, vec![VariableId(12), b]);
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
    let outcome = solver.solve(&graph, &options);

    match outcome.solution {
        Some(solution) => {
            assert!(solution.score.is_feasible());
            let count_3 = vars
                .iter()
                .filter(|v| solution.assignment.get(v) == Some(&3))
                .count();
            assert_eq!(count_3, 1);
            let count_1 = vars
                .iter()
                .filter(|v| solution.assignment.get(v) == Some(&1))
                .count();
            assert!(count_1 <= 2);
            let count_2 = vars
                .iter()
                .filter(|v| solution.assignment.get(v) == Some(&2))
                .count();
            assert!(count_2 >= 1);
        }
        None => panic!(
            "Expected feasible LNS solution under cardinality constraints, got status {:?}",
            outcome.status
        ),
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
    let outcome = solver.solve(&graph, &options);

    match outcome.solution {
        Some(solution) => assert!(solution.score.is_feasible()),
        None => panic!(
            "Expected feasible solution from ParallelSolver, got status {:?}",
            outcome.status
        ),
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
        shared_incumbent: None,
    };
    let outcome = ParallelSolver::new().solve(&graph, &options);
    assert!(
        matches!(outcome.status, SolveStatus::Aborted(_)),
        "expected Aborted since no worker could prove infeasibility with zero search budget, got {:?}",
        outcome.status
    );
}

#[test]
fn test_backtracking_aborts_on_node_limit() {
    let graph = build_nqueens_graph(4);
    let options = SolverOptions {
        time_limit: None,
        max_nodes: Some(0),
        cancellation_token: None,
        shared_incumbent: None,
    };
    let outcome = BacktrackingSolver::new().solve(&graph, &options);
    assert_eq!(outcome.status, SolveStatus::Aborted(AbortReason::NodeLimit));
    assert!(outcome.solution.is_none());
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
    let outcome = BacktrackingSolver::new().solve(&graph, &options);
    assert_eq!(outcome.status, SolveStatus::Aborted(AbortReason::Cancelled));
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

    let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    assert_eq!(
        outcome.status,
        SolveStatus::Optimal,
        "Branch & Bound should prove optimality for such a small instance"
    );
    let solution = outcome.solution.expect("Optimal implies a solution");
    assert_eq!(
        solution.score, oracle_best,
        "Branch & Bound optimum must match the brute-force oracle"
    );
    assert_eq!(
        outcome.bound,
        Some(oracle_best),
        "proven optimum: bound must equal the achieved score"
    );
}

#[test]
fn test_parallel_solver_returns_proven_optimum_not_first_worker_to_report() {
    // Exit criterion for plan/13-anytime-portfolio.md: same model as
    // `test_branch_and_bound_proves_optimum_matches_oracle` (a first-found assignment differs
    // from the true optimum), but through `ParallelSolver`. Backtracking/Local Search/LNS have
    // no reason to find the true maximum-sum assignment on their first try, and may report a
    // worse-but-feasible result before Branch & Bound (seeded via the shared incumbent) proves
    // the optimum -- the *final* result must still be the proven optimum, not whichever worker's
    // `SolveOutcome` happened to arrive first.
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..3)
        .map(|i| builder.new_var(format!("v{i}"), 1..=5))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_maximize(vars.clone(), 1);
    let graph = builder.build().expect("model should validate");

    let oracle_best = brute_force_best(&graph).expect("feasible by construction");
    assert_eq!(oracle_best, HardSoftScore::new(0, 12));

    let outcome = ParallelSolver::new().solve(&graph, &SolverOptions::default());
    assert_eq!(
        outcome.status,
        SolveStatus::Optimal,
        "Branch & Bound (one of the four portfolio workers) should prove optimality here"
    );
    let solution = outcome.solution.expect("Optimal implies a solution");
    assert_eq!(
        solution.score, oracle_best,
        "ParallelSolver must return the proven optimum, not a worse solution some other worker \
         may have reported first"
    );
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

    let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    assert_eq!(outcome.status, SolveStatus::Optimal);
    let solution = outcome.solution.expect("Optimal implies a solution");
    assert_eq!(solution.score, oracle_best);
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
        shared_incumbent: None,
    };
    let outcome = BranchAndBoundSolver::new().solve(&graph, &options);
    match outcome.status {
        SolveStatus::Feasible => {}
        SolveStatus::Aborted(_) => {} // also acceptable: no feasible solution found within budget
        other => panic!("Expected Feasible (not proven optimal) or Aborted, got {other:?}"),
    }
    assert_ne!(
        outcome.status,
        SolveStatus::Optimal,
        "3 search nodes cannot exhaust a 6-variable AllDifferent tree"
    );
}

#[test]
fn test_exactly_one_partial_cardinality_optimality_matches_oracle() {
    // Integration-level regression test for plan/09-project-reevaluation-roadmap.md P0
    // (unit-level version lives in src/solver/branch_and_bound.rs): a naive optimistic hard
    // bound derived from `ExactlyOne::is_satisfied` on a partial assignment would prune a
    // still-winnable branch, since `ExactlyOne` is `false` before any hit even though a later
    // assignment could still satisfy it.
    let mut builder = ModelBuilder::new();
    let x = builder.new_var("x", 0..=1);
    let y = builder.new_var("y", 0..=2);
    builder.add_exactly_one(vec![x, y], 0);
    builder.add_maximize(vec![x], 1);
    let graph = builder.build().expect("model should validate");

    let oracle_best = brute_force_best(&graph).expect("feasible: x=1,y=0 satisfies ExactlyOne");
    assert_eq!(oracle_best, HardSoftScore::new(0, 1));

    let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    assert_eq!(outcome.status, SolveStatus::Optimal);
    let solution = outcome.solution.expect("Optimal implies a solution");
    assert_eq!(solution.score, oracle_best);
    assert_eq!(solution.assignment[&x], 1);
    assert_eq!(solution.assignment[&y], 0);
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
    assert!(graph.finalize().is_err());
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

/// Regression: the portfolio must not report an **infeasible** assignment as a solution.
///
/// Pigeonhole — three variables, two values, all different — is unsatisfiable. `LocalSearch`
/// starts from a complete (and therefore conflicting) assignment and improves it; if it offers
/// those improvements to the shared incumbent, `ParallelSolver` reads one back at the end and
/// returns it as `Feasible`. A caller then stores a plan that violates hard constraints without
/// ever being told.
#[test]
fn portfolio_never_returns_an_infeasible_assignment() {
    let mut builder = ModelBuilder::new();
    let vars: Vec<VariableId> = (0..3)
        .map(|index| builder.new_var(format!("x{index}"), 1..=2))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_maximize(vars.clone(), 1);
    let graph = builder.build().expect("graph");

    let outcome = ParallelSolver::new().solve(
        &graph,
        &SolverOptions {
            time_limit: Some(Duration::from_millis(500)),
            ..SolverOptions::default()
        },
    );

    assert!(
        outcome.solution.is_none(),
        "unsatisfiable model returned a solution: {:?}",
        outcome.solution.map(|solution| solution.assignment),
    );
    assert_ne!(outcome.status, SolveStatus::Feasible);
}
