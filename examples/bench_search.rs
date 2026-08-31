//! Benchmark harness for measuring solver search throughput (nodes/sec) over a small
//! representative corpus.
//!
//! Used to establish a baseline before attempting performance work, per
//! `plan/09-project-reevaluation-roadmap.md`'s Meilenstein 0.2 guidance ("nach P0 messen und
//! angehen" — measure, then act) for reversible domains/trail instead of a full domain-map clone
//! per search node. Run again after such a change and compare nodes/sec.
//!
//! Run with `cargo run --release --example bench_search`. Release mode matters: debug-mode
//! timings are dominated by unoptimized allocation/clone overhead and do not reflect the costs
//! this benchmark exists to measure.

use std::sync::Arc;
use std::time::{Duration, Instant};
use unifier::constraint::{NotEqual, TaskDemand};
use unifier::dsl::ModelBuilder;
use unifier::solver::{
    BacktrackingSolver, BranchAndBoundSolver, SearchStatistics, SolveOutcome, SolverOptions,
};
use unifier::{ValidatedGraph, VariableId};

/// N-Queens(n): pure CSP, propagation- and backtracking-heavy.
fn nqueens(n: i64) -> ValidatedGraph {
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
    builder.build().expect("valid model")
}

/// AllDifferent + maximize(sum): COP, Branch & Bound bound-pruning-heavy (many branches opened
/// and pruned rather than propagated away outright).
fn all_different_maximize(n: i64, domain_max: i64) -> ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let vars: Vec<VariableId> = (0..n)
        .map(|i| builder.new_var(format!("v{i}"), 1..=domain_max))
        .collect();
    builder.add_all_different(vars.clone());
    builder.add_maximize(vars, 1);
    builder.build().expect("valid model")
}

/// A small resource-scheduling instance: Cumulative propagation over overlapping time windows.
fn cumulative_scheduling(n_tasks: i64, window: i64, capacity: u32) -> ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let starts: Vec<VariableId> = (0..n_tasks)
        .map(|i| builder.new_var(format!("t{i}_start"), 0..=window))
        .collect();
    let tasks = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| TaskDemand {
            start,
            duration: 2 + (i as u64 % 3),
            demand: 1,
        })
        .collect();
    builder.add_cumulative(tasks, capacity);
    builder.build().expect("valid model")
}

/// ExactlyOne + AtLeast + maximize: exercises the `Constraint::is_satisfiable` domain-sensitive
/// bound path added for the P0 optimality fix (see `plan/09-project-reevaluation-roadmap.md`).
fn exactly_one_and_at_least(n: i64, domain_max: i64) -> ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let vars: Vec<VariableId> = (0..n)
        .map(|i| builder.new_var(format!("v{i}"), 0..=domain_max))
        .collect();
    builder.add_exactly_one(vars.clone(), 0);
    builder.add_at_least(2, vars.clone(), 1);
    builder.add_maximize(vars, 1);
    builder.build().expect("valid model")
}

fn run_backtracking(name: &str, graph: &ValidatedGraph, options: &SolverOptions) {
    report(name, graph, options, |g, o| {
        BacktrackingSolver::new().solve(g, o)
    });
}

fn run_branch_and_bound(name: &str, graph: &ValidatedGraph, options: &SolverOptions) {
    report(name, graph, options, |g, o| {
        BranchAndBoundSolver::new().solve(g, o)
    });
}

fn report(
    name: &str,
    graph: &ValidatedGraph,
    options: &SolverOptions,
    solve: impl FnOnce(&ValidatedGraph, &SolverOptions) -> SolveOutcome,
) {
    let wall_start = Instant::now();
    let outcome = solve(graph, options);
    let wall = wall_start.elapsed();
    let SearchStatistics {
        nodes_expanded,
        elapsed,
    } = outcome.statistics;
    let nodes_per_sec = if elapsed.as_secs_f64() > 0.0 {
        nodes_expanded as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    println!(
        "{name:<34} status={:<11?} nodes={nodes_expanded:>9} elapsed={elapsed:>9.3?} wall={wall:>9.3?} nodes/sec={nodes_per_sec:>13.0}",
        outcome.status,
    );
}

fn main() {
    let options = SolverOptions {
        time_limit: Some(Duration::from_secs(20)),
        ..SolverOptions::default()
    };

    println!("=== unifier search benchmark ===");
    println!("(release mode matters: run with `cargo run --release --example bench_search`)\n");

    for n in [8, 10, 12] {
        run_backtracking(&format!("nqueens({n}) backtracking"), &nqueens(n), &options);
    }
    for n in [6, 8] {
        run_branch_and_bound(
            &format!("all_different_maximize({n}) B&B"),
            &all_different_maximize(n, n + 5),
            &options,
        );
    }
    run_branch_and_bound(
        "cumulative_scheduling(8) B&B",
        &cumulative_scheduling(8, 20, 3),
        &options,
    );
    run_branch_and_bound(
        "exactly_one_at_least(8) B&B",
        &exactly_one_and_at_least(8, 3),
        &options,
    );
}
