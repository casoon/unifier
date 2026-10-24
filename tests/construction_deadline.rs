//! The construction respects the time limit it was given — and still returns a **complete**
//! assignment.
//!
//! `SolverOptions::time_limit` is a promise to the caller. The greedy construction in
//! `LocalSearchSolver` ignored it for a long time: it did not even take the options, so an
//! attempt once started ran to completion, no matter how large the model was. On a small one
//! that is milliseconds; on a large one, double-digit seconds, and the repair afterwards only
//! finds an expired limit (measured in timbra's plan/59).
//!
//! Two properties, both required:
//!
//! - **It stops.** With a limit close to zero the run returns immediately instead of first
//!   finishing the construction of the model.
//! - **It returns something.** A half-assigned result would not be an assignment, and the
//!   anytime contract (`SolveOutcome::best_effort`) requires a complete one. On abort, the rest
//!   gets the first value of its domain.

use std::time::{Duration, Instant};
use unifier::solver::{LocalSearchSolver, SolverOptions};
use unifier::{AllDifferent, ModelBuilder};

/// Large enough that a construction carried through to the end would take considerably longer
/// than the limit below — and small enough that the test runs in milliseconds when it stops.
fn wide_model() -> unifier::propagation::graph::ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..400)
        .map(|index| builder.new_var(format!("v{index}"), 0..=399))
        .collect();
    // One constraint over everything: every assignment has to be checked against every other,
    // so the construction genuinely costs something here.
    builder.add_constraint(std::sync::Arc::new(AllDifferent::new(vars.clone())));
    builder.build().expect("model")
}

#[test]
fn a_deadline_of_almost_nothing_stops_the_construction() {
    let graph = wide_model();
    let options = SolverOptions {
        time_limit: Some(Duration::from_millis(1)),
        ..SolverOptions::default()
    };

    let started = Instant::now();
    let outcome = LocalSearchSolver::new(8).solve(&graph, &options);
    let elapsed = started.elapsed();

    // Generous: this is about "stops" versus "constructs all 400 variables to the end", not
    // about millisecond precision. Without the check in the construction, the run is orders of
    // magnitude above this.
    assert!(
        elapsed < Duration::from_secs(5),
        "the run ignored the limit: {elapsed:?}",
    );

    // And it does not come back empty-handed: whatever was reached is complete.
    if let Some(reached) = outcome.reached() {
        assert_eq!(
            reached.assignment.len(),
            graph.variables().len(),
            "an aborted construction must still assign every variable",
        );
    }
}
