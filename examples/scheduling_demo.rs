//! Demonstration of Resource-Constrained Timetabling & Scheduling using `unifier`.
//!
//! A school timetable over an 8-slot horizon exercising every piece of the scheduling vertical
//! (`plan/12-scheduling-vertical.md`):
//! - Point 2: `Activity`/`Resource` compiled automatically via
//!   `ModelBuilder::compile_scheduling_model` — `Cumulative` for the shared room (capacity 2),
//!   `NoOverlap` for the unary teacher.
//! - 3a: a calendar restriction (Math can't start during a school-assembly slot).
//! - 3b: an optional "Study Hall" activity (`add_optional`) that only occupies the room if the
//!   solver decides to include it.
//! - 3c: Chemistry's lab is an *alternative* resource choice (`add_alternative_resources`) —
//!   Lab A or Lab B, whichever is free.
//! - 3d: a tardiness objective on Chemistry's finish time against a preferred deadline.
//!
//! Solution inspection prints a simple text-Gantt bar per activity instead of raw slot numbers.

use std::sync::Arc;
use unifier::constraint::{Cumulative, TaskDemand};
use unifier::dsl::ModelBuilder;
use unifier::model::activity::Activity;
use unifier::solver::{BranchAndBoundSolver, SolveStatus, SolverOptions};

const HORIZON: i64 = 7;

/// Renders `[start, end)` as a `HORIZON`-wide text bar, or a plain "(absent)" marker.
fn gantt_bar(range: Option<(i64, i64)>) -> String {
    match range {
        None => "(absent)".to_string(),
        Some((start, end)) => (0..=HORIZON)
            .map(|t| if t >= start && t < end { '#' } else { '.' })
            .collect(),
    }
}

fn main() {
    println!("=== Unifier CSP/COP Scheduling Demo ===");

    let mut builder = ModelBuilder::new();

    // Durations
    let math_dur = 2u64;
    let physics_dur = 1u64;
    let chemistry_dur = 2u64;
    let study_hall_dur = 1u64;

    // Room: shared capacity for 2 concurrent lessons -> compiles to `Cumulative`.
    let room = builder.new_resource("room", 2);
    // Teacher Müller: a unary resource (one lesson at a time) -> compiles to `NoOverlap`.
    let teacher_mueller = builder.new_resource("teacher_mueller", 1);
    // Chemistry's lab is an alternative between two unary labs (3c).
    let lab_a = builder.new_resource("lab_a", 1);
    let lab_b = builder.new_resource("lab_b", 1);

    let math_interval = builder.new_interval("math", 0..=HORIZON, math_dur, 0..=(HORIZON + 2));
    let mut math = builder.new_activity("Math", math_interval);
    math.require_resource(room.id(), 1);
    math.require_resource(teacher_mueller.id(), 1);
    // 3a: the school assembly blocks slot 3 -- Math can't start there.
    builder.add_calendar(math.interval().start(), &[(3, 3)]);

    let physics_interval =
        builder.new_interval("physics", 0..=HORIZON, physics_dur, 0..=(HORIZON + 1));
    let mut physics = builder.new_activity("Physics", physics_interval);
    physics.require_resource(room.id(), 1);
    physics.require_resource(teacher_mueller.id(), 1);

    let chemistry_interval =
        builder.new_interval("chemistry", 0..=HORIZON, chemistry_dur, 0..=(HORIZON + 2));
    let mut chemistry = builder.new_activity("Chemistry", chemistry_interval);
    chemistry.require_resource(room.id(), 1);
    // Chemistry's lab is chosen via `add_alternative_resources` below, *not* `require_resource`
    // (see that method's doc comment: registering it both ways would double-constrain it).

    // Mandatory activities compile normally: room (Cumulative) + teacher (NoOverlap).
    let mandatory_activities = [math.clone(), physics.clone(), chemistry.clone()];
    let resources = [room.clone(), teacher_mueller];
    builder
        .compile_scheduling_model(&mandatory_activities, &resources, &[])
        .expect("scheduling model compiles: every resource/activity reference is valid");

    // 3c: Chemistry runs in Lab A or Lab B, whichever is free (no other activity needs either
    // lab here, so this mostly demonstrates the exactly-one-alternative bookkeeping itself).
    let lab_resources = [lab_a.clone(), lab_b.clone()];
    let lab_presences = builder
        .add_alternative_resources(
            &chemistry,
            &[(lab_a.id(), 1), (lab_b.id(), 1)],
            std::slice::from_ref(&chemistry),
            &lab_resources,
        )
        .expect("valid alternative-resource model");

    // 3b: Study Hall is optional -- present only if the solver finds room for it. Deliberately
    // *not* passed to `compile_scheduling_model` (that would make its room usage mandatory);
    // instead, a single `Optional`-gated `Cumulative` constraint over Study Hall *plus every*
    // mandatory room-user models its conditional participation. This must be one combined N-way
    // constraint, not one pairwise constraint per other activity: with room capacity 2 and three
    // other room-users, a pairwise capacity check between Study Hall and each one individually
    // (1 + 1 = 2 <= 2) would never catch three of them coinciding at once (1 + 1 + 1 = 3 > 2) --
    // see `ModelBuilder::add_alternative_resources`'s doc comment for the same pitfall.
    let study_hall_interval =
        builder.new_interval("study_hall", 0..=HORIZON, study_hall_dur, 0..=(HORIZON + 1));
    let study_hall = builder.new_activity("Study Hall", study_hall_interval);
    let study_hall_present = builder.new_presence_var("study_hall_present");
    let study_hall_and_room_users = Cumulative::new(
        vec![
            TaskDemand {
                start: study_hall.interval().start(),
                duration: study_hall_dur,
                demand: 1,
            },
            TaskDemand {
                start: math.interval().start(),
                duration: math_dur,
                demand: 1,
            },
            TaskDemand {
                start: physics.interval().start(),
                duration: physics_dur,
                demand: 1,
            },
            TaskDemand {
                start: chemistry.interval().start(),
                duration: chemistry_dur,
                demand: 1,
            },
        ],
        room.capacity(),
    );
    builder.add_optional(Arc::new(study_hall_and_room_users), study_hall_present);

    // 3d: Chemistry should ideally finish by slot 5 -- minimize tardiness beyond that (weighted
    // higher than the secondary preference below, so it's resolved first), with a small bonus
    // for including Study Hall when the schedule has room for it.
    let tardiness_vars = builder.add_tardiness_minimize(&[(chemistry.interval().end(), 5)], 10);
    builder.add_maximize([study_hall_present], 1);

    let graph = builder.build().expect("model should validate");
    println!("Model built with {} variables.", graph.variables().len());

    // Branch & Bound (not the Parallel Portfolio solver): this model has soft objectives
    // (tardiness minimization, Study Hall preference) and only Branch & Bound proves optimality
    // in this crate -- worth showing a genuinely optimal, not just feasible, schedule here.
    println!("Running Branch & Bound Solver...");
    let solver = BranchAndBoundSolver::new();
    let options = SolverOptions::default();

    let outcome = solver.solve(&graph, &options);
    match (outcome.status, outcome.solution) {
        (SolveStatus::Optimal | SolveStatus::Feasible, Some(solution)) => {
            println!("✅ Feasible Schedule Found! (status: {:?})", outcome.status);
            println!("Score: {}\n", solution.score);

            let interval_range = |a: &Activity| {
                (
                    solution.assignment[&a.interval().start()],
                    solution.assignment[&a.interval().end()],
                )
            };

            println!("Schedule (horizon 0..={HORIZON}):");
            for (name, range) in [
                ("Math", Some(interval_range(&math))),
                ("Physics", Some(interval_range(&physics))),
                ("Chemistry", Some(interval_range(&chemistry))),
            ] {
                println!("  {name:<12} {}", gantt_bar(range));
            }

            let study_hall_active = solution.assignment[&study_hall_present] == 1;
            let study_hall_range = study_hall_active.then(|| interval_range(&study_hall));
            println!("  {:<12} {}", "Study Hall", gantt_bar(study_hall_range));

            let chosen_lab = if solution.assignment[&lab_presences[0]] == 1 {
                "Lab A"
            } else {
                "Lab B"
            };
            println!("\nChemistry's lab: {chosen_lab}");
            println!(
                "Chemistry tardiness (deadline 5): {}",
                solution.assignment[&tardiness_vars[0]]
            );
            println!(
                "Study Hall included: {}",
                if study_hall_active { "yes" } else { "no" }
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
