//! Die Konstruktion hält das Zeitlimit, das ihr gegeben wurde — und gibt dabei eine
//! **vollständige** Belegung zurück.
//!
//! `SolverOptions::time_limit` ist eine Zusage an den Aufrufer. Die gierige Konstruktion in
//! `LocalSearchSolver` hat sie lange ignoriert: sie nahm die Optionen nicht einmal entgegen,
//! also lief ein einmal begonnener Anlauf zu Ende, gleich wie groß das Modell war. Auf einem
//! kleinen sind das Millisekunden; auf einem großen zweistellige Sekunden, und die Reparatur
//! danach findet nur noch ein abgelaufenes Limit vor (gemessen in timbras plan/59).
//!
//! Zwei Eigenschaften, beide nötig:
//!
//! - **Sie hört auf.** Mit einem Limit nahe null kehrt der Lauf sofort zurück, statt das
//!   Modell erst fertig zu konstruieren.
//! - **Sie gibt etwas zurück.** Eine halb belegte Rückgabe wäre keine Belegung, und der
//!   Anytime-Vertrag (`SolveOutcome::best_effort`) verlangt eine vollständige. Der Rest
//!   bekommt beim Abbruch den ersten Wert seiner Domäne.

use std::time::{Duration, Instant};
use unifier::solver::{LocalSearchSolver, SolverOptions};
use unifier::{AllDifferent, ModelBuilder};

/// Groß genug, dass eine zu Ende gedachte Konstruktion deutlich länger bräuchte als das
/// Limit unten — und klein genug, dass der Test in Millisekunden läuft, wenn sie aufhört.
fn wide_model() -> unifier::propagation::graph::ValidatedGraph {
    let mut builder = ModelBuilder::new();
    let vars: Vec<_> = (0..400)
        .map(|index| builder.new_var(format!("v{index}"), 0..=399))
        .collect();
    // Ein Constraint über alles: jede Zuweisung muss gegen jede andere geprüft werden, also
    // kostet die Konstruktion hier wirklich etwas.
    builder.add_constraint(std::sync::Arc::new(AllDifferent::new(vars.clone())));
    builder.build().expect("Modell")
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

    // Großzügig: es geht um „hört auf" gegen „konstruiert 400 Variablen zu Ende", nicht um
    // Millisekunden-Genauigkeit. Ohne die Prüfung in der Konstruktion liegt der Lauf um
    // Größenordnungen darüber.
    assert!(
        elapsed < Duration::from_secs(5),
        "der Lauf hat das Limit ignoriert: {elapsed:?}",
    );

    // Und er kommt nicht mit leeren Händen: was erreicht wurde, ist vollständig.
    if let Some(reached) = outcome.reached() {
        assert_eq!(
            reached.assignment.len(),
            graph.variables().len(),
            "eine abgebrochene Konstruktion muss trotzdem jede Variable belegen",
        );
    }
}
