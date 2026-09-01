//! Criterion micro-benchmarks for `unifier`'s solvers.
//!
//! Complements `examples/bench_search.rs` (a manual nodes/sec harness kept for larger,
//! release-mode-only corpus runs) with a `cargo bench`-integrated suite for quick, statistically
//! sound regression checks on representative CSP/COP model shapes: pure CSP backtracking
//! (N-Queens), COP Branch & Bound with bound-pruning (AllDifferent + maximize), and a global
//! scheduling constraint (Cumulative).
//!
//! Run with `cargo bench`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::sync::Arc;
use unifier::constraint::{NotEqual, TaskDemand};
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, BranchAndBoundSolver, SolverOptions};
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

/// AllDifferent + maximize(sum): COP, Branch & Bound bound-pruning-heavy.
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

fn bench_nqueens(c: &mut Criterion) {
    let mut group = c.benchmark_group("N-Queens (Backtracking)");
    for n in [8, 10, 12].iter() {
        let graph = nqueens(*n);
        let options = SolverOptions::default();
        group.bench_with_input(BenchmarkId::new("n", n), n, |b, _| {
            b.iter(|| BacktrackingSolver::new().solve(&graph, &options))
        });
    }
    group.finish();
}

fn bench_all_different_maximize(c: &mut Criterion) {
    let mut group = c.benchmark_group("AllDifferent + Maximize (Branch & Bound)");
    // A tight domain (n + 5, matching `examples/bench_search.rs`'s known-fast parameterization)
    // keeps each iteration in the millisecond range — Criterion runs many repeated samples, so
    // this deliberately stays far smaller than `bench_search.rs`'s larger, single-run corpus
    // cases (e.g. `all_different_maximize(9, 14)`, which alone runs for seconds).
    for n in [5, 6].iter() {
        let graph = all_different_maximize(*n, n + 5);
        let options = SolverOptions::default();
        group.bench_with_input(BenchmarkId::new("n", n), n, |b, _| {
            b.iter(|| BranchAndBoundSolver::new().solve(&graph, &options))
        });
    }
    group.finish();
}

fn bench_cumulative_scheduling(c: &mut Criterion) {
    let mut group = c.benchmark_group("Cumulative Scheduling (Branch & Bound)");
    for n_tasks in [8, 12].iter() {
        let graph = cumulative_scheduling(*n_tasks, 16, 2);
        let options = SolverOptions::default();
        group.bench_with_input(BenchmarkId::new("n_tasks", n_tasks), n_tasks, |b, _| {
            b.iter(|| BranchAndBoundSolver::new().solve(&graph, &options))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_nqueens,
    bench_all_different_maximize,
    bench_cumulative_scheduling
);
criterion_main!(benches);
