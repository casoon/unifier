//! A small job-shop scheduling problem as a constraint optimization problem (COP): three jobs,
//! three machines, minimize the makespan. Solved with `BranchAndBoundSolver`, which proves the
//! result optimal.
//!
//! Each job is a fixed sequence of operations (`Precedence`), each machine runs one operation at
//! a time (`NoOverlap`), and a makespan variable bounds every job's last end
//! (`LessThanOrEqual`) and is minimized.
//!
//! Run with `cargo run --example job_shop`.

use unifier::dsl::ModelBuilder;
use unifier::solver::{BranchAndBoundSolver, SolverOptions};
use unifier::{Interval, VariableId};

/// `(machine, duration)` per operation, in the order each job must run them.
const JOBS: [&[(usize, u64)]; 3] = [
    &[(0, 3), (1, 2), (2, 2)],
    &[(0, 2), (2, 1), (1, 4)],
    &[(1, 4), (2, 3)],
];
const MACHINES: usize = 3;

fn main() {
    let horizon: i64 = JOBS
        .iter()
        .flat_map(|job| job.iter())
        .map(|&(_, d)| d as i64)
        .sum();

    let mut builder = ModelBuilder::new();
    let mut per_machine: Vec<Vec<(usize, Interval, u64)>> = vec![Vec::new(); MACHINES];
    let mut last_ends: Vec<VariableId> = Vec::new();

    for (job, operations) in JOBS.iter().enumerate() {
        let mut previous: Option<Interval> = None;
        for (index, &(machine, duration)) in operations.iter().enumerate() {
            let op = builder.new_interval(
                &format!("j{job}_op{index}"),
                0..=horizon,
                duration,
                0..=horizon,
            );
            if let Some(previous) = &previous {
                builder.add_precedence(previous, &op, 0);
            }
            per_machine[machine].push((job, op.clone(), duration));
            previous = Some(op);
        }
        last_ends.push(previous.expect("every job has operations").end());
    }

    for operations in &per_machine {
        let intervals: Vec<Interval> = operations.iter().map(|(_, op, _)| op.clone()).collect();
        let durations: Vec<u64> = operations.iter().map(|&(_, _, d)| d).collect();
        builder.add_no_overlap(&intervals, &durations);
    }

    let makespan = builder.new_var("makespan", 0..=horizon);
    for &end in &last_ends {
        builder.add_less_than_or_equal(end, makespan, 0);
    }
    builder.add_minimize([makespan], 1);

    let graph = builder.build().expect("job-shop model validates");
    println!(
        "Job shop: {} jobs, {MACHINES} machines, horizon {horizon}",
        JOBS.len()
    );

    let outcome = BranchAndBoundSolver::new().solve(&graph, &SolverOptions::default());
    let Some(solution) = outcome.solution else {
        println!("No schedule: {:?}", outcome.status);
        return;
    };
    let length = solution.assignment[&makespan];
    println!("Status: {:?}", outcome.status);
    println!("Score: {}", solution.score);
    println!("Makespan: {length}\n");

    println!(
        "       {}",
        (0..length)
            .map(|t| (t % 10).to_string())
            .collect::<String>()
    );
    for (machine, operations) in per_machine.iter().enumerate() {
        let mut row = vec!['.'; length as usize];
        for (job, op, duration) in operations {
            let start = solution.assignment[&op.start()];
            for t in start..start + *duration as i64 {
                row[t as usize] = char::from_digit(*job as u32, 10).expect("single-digit job");
            }
        }
        println!("  M{machine}   {}", row.into_iter().collect::<String>());
    }
    println!("\nDigits are job numbers; each row is one machine.");
}
