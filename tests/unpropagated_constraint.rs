//! Eine Constraint ohne Propagator darf die Baumsuche nicht den ganzen Teilbaum kosten.
//!
//! `BacktrackingSolver` hat lange nur am Blatt gefragt, ob eine Belegung zulässig ist.
//! Solange jede Constraint propagiert, fällt das nicht auf: ein Widerspruch zeigt sich
//! als Konflikt in dem Knoten, der ihn verursacht. Eine Constraint, deren `propagate`
//! nichts sagt, meldet sich dagegen erst, wenn *alles* belegt ist — und liegt unter der
//! Entscheidung, die sie verletzt, eine Variable mit großem Wertebereich, wird jeder
//! einzelne Wert durchprobiert, bevor die Suche zur Ursache zurückkehrt.
//!
//! Genau das ist in `schedulr` passiert (dessen `SelectedResourceCapacity` propagierte
//! nie): 1.116.987 Knoten für eine einzige Aktivität. `BranchAndBoundSolver` hatte das
//! Problem nie, weil seine Schranke über `optimistic_score` jede Constraint nach
//! `is_satisfiable` fragt. Die Baumsuche fragt jetzt dasselbe, in jedem Knoten, für die
//! Constraints der gerade belegten Variable.

use std::collections::HashMap;
use std::sync::Arc;
use unifier::constraint::PropagationResult;
use unifier::{
    BacktrackingSolver, Constraint, ModelBuilder, SolveStatus, SolverOptions, TrailedDomains,
    VariableId,
};

/// Genau eine der beiden Variablen ist 1 — geprüft, aber nie propagiert.
///
/// Die Wertereihenfolge probiert 0 zuerst, also beginnt die Suche mit `a = 0, b = 0`:
/// verletzt, sobald beide belegt sind, aber `propagate` sagt es nicht.
#[derive(Debug)]
struct ExactlyOneSilently {
    scope: Vec<VariableId>,
}

impl Constraint for ExactlyOneSilently {
    fn name(&self) -> &str {
        "ExactlyOneSilently"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match (
            assignment.get(&self.scope[0]),
            assignment.get(&self.scope[1]),
        ) {
            (Some(a), Some(b)) => a + b == 1,
            // Partielle Belegung: noch nicht verletzt, solange etwas fehlt — der Vertrag
            // aus `tests/partial_assignment.rs`.
            _ => true,
        }
    }

    fn propagate(&self, _domains: &mut TrailedDomains) -> PropagationResult {
        PropagationResult::Success { changed: false }
    }
}

#[test]
fn a_violation_is_caught_where_it_happens_not_at_the_leaf() {
    let mut builder = ModelBuilder::new();
    let a = builder.new_var("a", 0..=1);
    let b = builder.new_var("b", 0..=1);
    // Groß genug, dass ein Durchlauf pro verworfener Entscheidung auffällt; frei von
    // Constraints, also gäbe es für jeden ihrer Werte eine Lösung, sobald a und b stimmen.
    let _wide = builder.new_var("wide", 0..=20_000);
    builder.add_constraint(Arc::new(ExactlyOneSilently { scope: vec![a, b] }));
    let graph = builder.build().expect("valid model");

    let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());

    assert_eq!(outcome.status, SolveStatus::Feasible);
    let solution = outcome.solution.expect("a feasible outcome has a solution");
    assert_eq!(solution.assignment[&a] + solution.assignment[&b], 1);
    // Vorher: die Suche belegt a = 0, b = 0, dann `wide` — und probiert alle 20.001
    // Werte, jeden bis zum Blatt, bevor sie b = 1 versucht. Die Grenze liegt weit über
    // dem, was eine Suche braucht, die den Widerspruch im Knoten sieht, und weit unter
    // dem, was sie ohne das braucht.
    assert!(
        outcome.statistics.nodes_expanded < 100,
        "{} Knoten — der Widerspruch wurde erst am Blatt bemerkt",
        outcome.statistics.nodes_expanded
    );
}
