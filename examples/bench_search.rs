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
    BacktrackingSolver, BranchAndBoundSolver, ParallelSolver, SearchStatistics, SolveOutcome,
    SolverOptions,
};
use unifier::{Interval, ValidatedGraph, VariableId};

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

/// A small disjunctive (unary-resource) scheduling instance: `NoOverlap` over task intervals
/// (each fully occupies the resource while active — e.g. one teacher/room/machine), independent
/// of `Cumulative`. Added alongside part D (`plan/11-search-heuristics-and-global-constraints.md`)
/// so the energetic-reasoning overload check gets its own measured baseline instead of only being
/// exercised indirectly through `cumulative_scheduling`.
fn no_overlap_scheduling(n_tasks: i64, window: i64) -> ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let durations: Vec<u64> = (0..n_tasks).map(|i| 2 + (i as u64 % 3)).collect();
    let intervals: Vec<Interval> = (0..n_tasks)
        .map(|i| {
            let duration = durations[i as usize];
            builder.new_interval(
                &format!("t{i}"),
                0..=window,
                duration,
                0..=(window + duration as i64),
            )
        })
        .collect();
    builder.add_no_overlap(&intervals, &durations);
    builder.build().expect("valid model")
}

/// A small job-shop-style instance: `n_jobs` jobs, each a fixed sequence of `n_machines`
/// operations (one per machine, in a job-specific order — a cyclic permutation of machines, so
/// jobs genuinely contend for machines in different orders rather than all queuing identically).
/// Combines `Precedence` (each job's operations run in sequence) with `NoOverlap` (each machine
/// is a unary resource) — real cross-constraint contention structure, unlike
/// `cumulative_scheduling`/`no_overlap_scheduling`'s independent, uniformly-random task domains.
///
/// Modeled after classic job-shop scheduling benchmarks (e.g. Fisher, H., & Thompson, G. L.
/// (1963). *Probabilistic learning combinations of local job-shop scheduling rules*, the origin
/// of the "FT" instance family), scaled down. Added per
/// `plan/11-search-heuristics-and-global-constraints.md`, part D: the existing corpus is
/// synthetic/randomly generated and doesn't exercise the multi-constraint resource contention a
/// real scheduling instance would, making it a weak signal for whether `Cumulative`/`NoOverlap`
/// edge-finding actually helps in practice.
///
/// The horizon (`start`/`end` domain upper bound) is set to the makespan lower bound — the
/// longer of the busiest single job's total duration and the busiest machine's total load — plus
/// one job's worth of slack, keeping the instance feasible but tight enough to require real
/// search rather than being solvable by propagation alone.
fn job_shop_scheduling(n_jobs: i64, n_machines: i64) -> ValidatedGraph {
    let mut builder = ModelBuilder::new();

    // Deterministic per-(job, machine-slot) duration: varies enough to avoid a degenerate
    // uniform instance without needing true randomness (benchmark runs must stay reproducible).
    let duration_at = |job: i64, slot: i64| -> u64 { 2 + ((job * 3 + slot * 2) % 4) as u64 };

    let longest_job: i64 = (0..n_jobs)
        .map(|job| {
            (0..n_machines)
                .map(|slot| duration_at(job, slot) as i64)
                .sum()
        })
        .max()
        .unwrap_or(0);
    let busiest_machine: i64 = (0..n_machines)
        .map(|machine| {
            (0..n_jobs)
                .map(|job| {
                    // Job `job`'s operation on `machine` is at the slot solving
                    // `(slot + job) % n_machines == machine` (see the assignment below).
                    let slot = (machine - job).rem_euclid(n_machines);
                    duration_at(job, slot) as i64
                })
                .sum()
        })
        .max()
        .unwrap_or(0);
    let horizon = longest_job.max(busiest_machine) + longest_job;

    let mut machine_ops: Vec<Vec<Interval>> = vec![Vec::new(); n_machines as usize];
    let mut machine_durations: Vec<Vec<u64>> = vec![Vec::new(); n_machines as usize];

    for job in 0..n_jobs {
        let mut prev_op: Option<Interval> = None;
        for slot in 0..n_machines {
            let duration = duration_at(job, slot);
            let op = builder.new_interval(
                &format!("j{job}_op{slot}"),
                0..=horizon,
                duration,
                0..=(horizon + duration as i64),
            );
            if let Some(prev) = &prev_op {
                builder.add_precedence(prev, &op, 0);
            }
            // Cyclic permutation: job `job`'s `slot`-th operation runs on machine
            // `(slot + job) % n_machines`, so different jobs visit machines in different orders.
            let machine = ((slot + job) % n_machines) as usize;
            machine_ops[machine].push(op.clone());
            machine_durations[machine].push(duration);
            prev_op = Some(op);
        }
    }

    for (ops, durations) in machine_ops.iter().zip(machine_durations.iter()) {
        builder.add_no_overlap(ops, durations);
    }

    builder.build().expect("valid job-shop model")
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

fn run_parallel(name: &str, graph: &ValidatedGraph, options: &SolverOptions) {
    report(name, graph, options, |g, o| {
        ParallelSolver::new().solve(g, o)
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

    // Small/medium cases (original corpus, kept for before/after comparability).
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
    // Same model as `all_different_maximize(6) B&B` above, through the 4-worker portfolio
    // instead: measures Meilenstein 0.4's join-overhead (every worker thread is now joined
    // before `solve()` returns, not left running in the background) head-to-head against
    // single-solver B&B on identical work. `nodes_expanded` sums across all four workers, so
    // nodes/sec isn't directly comparable to a sequential solver's — `wall` is the metric that
    // matters here.
    run_parallel(
        "all_different_maximize(6) Parallel",
        &all_different_maximize(6, 11),
        &options,
    );
    run_branch_and_bound(
        "cumulative_scheduling(8) B&B",
        &cumulative_scheduling(8, 20, 3),
        &options,
    );
    run_branch_and_bound(
        "no_overlap_scheduling(8) B&B",
        &no_overlap_scheduling(8, 20),
        &options,
    );
    run_branch_and_bound(
        "exactly_one_at_least(8) B&B",
        &exactly_one_and_at_least(8, 3),
        &options,
    );
    run_branch_and_bound(
        "job_shop_scheduling(3,3) B&B",
        &job_shop_scheduling(3, 3),
        &options,
    );

    // Larger/denser cases, added to measure how each category scales (per
    // plan/10-meilenstein-0.2-follow-up.md). `all_different_maximize(9)` in particular runs into
    // the tens of millions of nodes and takes on the order of 10+ seconds.
    println!();
    run_backtracking("nqueens(20) backtracking [larger]", &nqueens(20), &options);
    run_branch_and_bound(
        "all_different_maximize(9) B&B [larger]",
        &all_different_maximize(9, 14),
        &options,
    );
    run_branch_and_bound(
        "cumulative_scheduling(16) B&B [denser]",
        &cumulative_scheduling(16, 20, 3),
        &options,
    );
    run_branch_and_bound(
        // Unlike `cumulative_scheduling`, `NoOverlap` is a unary (capacity-1) resource: 16 tasks
        // with durations 2-4 (sum 47) need a horizon on that order to stay feasible-but-tight,
        // not the 20-slot window `cumulative_scheduling` uses at capacity 3.
        "no_overlap_scheduling(16) B&B [denser]",
        &no_overlap_scheduling(16, 45),
        &options,
    );
    run_branch_and_bound(
        "exactly_one_at_least(16) B&B [larger]",
        &exactly_one_and_at_least(16, 3),
        &options,
    );
    run_branch_and_bound(
        "job_shop_scheduling(4,4) B&B [denser]",
        &job_shop_scheduling(4, 4),
        &options,
    );
}
