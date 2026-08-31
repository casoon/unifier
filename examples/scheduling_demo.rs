//! Demonstration of Resource-Constrained Timetabling & Scheduling using `unifier`.
//!
//! Models a school timetable where 3 lessons (Math, Physics, Chemistry) must be scheduled
//! into 5 time slots across 2 available classrooms and 2 teachers without resource overlaps.

use unifier::constraint::TaskDemand;
use unifier::dsl::ModelBuilder;
use unifier::solver::{ParallelSolver, SolveStatus, SolverOptions};

fn main() {
    println!("=== Unifier CSP/COP Scheduling Demo ===");

    let mut builder = ModelBuilder::new();

    // Time slots 0..=4
    let math_start = builder.new_var("math_start", 0..=3);
    let physics_start = builder.new_var("physics_start", 0..=3);
    let chemistry_start = builder.new_var("chemistry_start", 0..=3);

    // Durations
    let math_dur = 2u64;
    let physics_dur = 1u64;
    let chemistry_dur = 2u64;

    // Resource demand on shared room capacity (max 2 concurrent lessons)
    let room_demands = vec![
        TaskDemand {
            start: math_start,
            duration: math_dur,
            demand: 1,
        },
        TaskDemand {
            start: physics_start,
            duration: physics_dur,
            demand: 1,
        },
        TaskDemand {
            start: chemistry_start,
            duration: chemistry_dur,
            demand: 1,
        },
    ];
    builder.add_cumulative(room_demands, 2);

    // Teacher demand: Math and Physics share Teacher Müller (unary resource -> NoOverlap)
    let teacher_mueller_intervals = [
        builder.new_interval("math", 0..=3, math_dur, 2..=5),
        builder.new_interval("physics", 0..=3, physics_dur, 1..=4),
    ];
    builder.add_no_overlap(&teacher_mueller_intervals, &[math_dur, physics_dur]);

    let graph = builder.build().expect("model should validate");
    println!("Model built with {} variables.", graph.variables().len());

    println!("Running Parallel Portfolio Solver (Backtracking + Local Search + LNS)...");
    let solver = ParallelSolver::new();
    let options = SolverOptions::default();

    let outcome = solver.solve(&graph, &options);
    match (outcome.status, outcome.solution) {
        (SolveStatus::Optimal | SolveStatus::Feasible, Some(solution)) => {
            println!("✅ Feasible Schedule Found!");
            println!("Score: {}", solution.score);
            println!("Schedule Assignment:");
            println!("  Math start: Slot {}", solution.assignment[&math_start]);
            println!(
                "  Physics start: Slot {}",
                solution.assignment[&physics_start]
            );
            println!(
                "  Chemistry start: Slot {}",
                solution.assignment[&chemistry_start]
            );
        }
        (SolveStatus::Infeasible, _) => {
            println!("❌ Problem is Infeasible");
        }
        (SolveStatus::Aborted(reason), _) => {
            println!("⏰ Search Aborted ({reason:?})");
        }
        (status, None) => {
            println!("⚠️ Unexpected outcome: {status:?} without a solution");
        }
    }
}
