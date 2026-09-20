//! Der Vertrag, den `Constraint::is_satisfied` unter einer **partiellen** Belegung zusagt:
//! `false` nur, wenn das bereits Belegte das Constraint verletzt — nicht schon, weil noch
//! etwas fehlt.
//!
//! Das steht so in der Dokumentation der Methode („or if unassigned variables do not violate
//! the constraint yet"), und die meisten Constraints halten es: `AllDifferent` sieht nur die
//! belegten Variablen, `Equal` vergleicht nur, wenn beide Seiten da sind, `AtMost` zählt
//! nach oben. `ExactlyOne` (`== 1`) und `AtLeast` (`>= k`) taten es nicht — sie meldeten eine
//! Gruppe als verletzt, in der noch niemand das Ziel belegt hatte, obwohl jede Variable darin
//! es noch werden konnte.
//!
//! Warum das nicht kosmetisch ist: die gierige Konstruktion in `LocalSearchSolver` setzt eine
//! Variable nur dann ohne Rücknahme, wenn *kein* Constraint verletzt ist. Eine Variable in
//! einer noch unberührten Auswahlgruppe konnte diese Bedingung nie erfüllen und lief deshalb
//! in die Rücknahme — samt der Variablen, die sie mitnahm. Gemessen in plan/58, K3.
//!
//! Auf einer **vollständigen** Belegung ändert der Vertrag nichts, und genau das prüfen die
//! Tests hier mit: dort ist nichts unbelegt, und beide Constraints fallen auf ihre alte
//! Bedingung zurück. Der harte Score, der über vollständige Belegungen summiert, sieht
//! dieselben Zahlen wie vorher.

use std::collections::HashMap;
use unifier::VariableId;
use unifier::constraint::{AtLeast, AtMost, Constraint, ExactlyOne};

fn vars(count: usize) -> Vec<VariableId> {
    (0..count).map(|index| VariableId(index as u32)).collect()
}

fn assign(pairs: &[(VariableId, i64)]) -> HashMap<VariableId, i64> {
    pairs.iter().copied().collect()
}

#[test]
fn exactly_one_waits_for_the_group_instead_of_crying_early() {
    let scope = vars(3);
    let constraint = ExactlyOne::new(scope.clone(), 1);

    // Nichts belegt: jede der drei kann noch die eine werden.
    assert!(constraint.is_satisfied(&assign(&[])));

    // Eine Null belegt, zwei offen: immer noch erreichbar.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0)])));

    // Die eine steht — erfüllt, egal was noch kommt.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1)])));

    // Zwei stehen auf dem Ziel: das kommt nicht mehr herunter, also verletzt, auch partiell.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1)])));

    // Vollständig belegt und keine auf dem Ziel: verletzt. Hier fällt der Vertrag auf die
    // alte Bedingung zurück, und der harte Score zählt wie zuvor.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 0), (scope[2], 0)])));
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 1), (scope[2], 0)])));
}

#[test]
fn at_least_waits_until_the_count_is_out_of_reach() {
    let scope = vars(3);
    let constraint = AtLeast::new(2, scope.clone(), 1);

    assert!(constraint.is_satisfied(&assign(&[])));

    // Eine daneben, zwei offen — zwei sind noch erreichbar.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0)])));

    // Zwei daneben, eine offen: zwei sind nicht mehr erreichbar.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 0)])));

    // Vollständig, zwei auf dem Ziel: erfüllt.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1), (scope[2], 0)])));
    // Vollständig, eine auf dem Ziel: verletzt.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 0), (scope[2], 0)])));
}

/// `AtMost` hielt den Vertrag schon immer — hier als Gegenprobe, damit die drei
/// Kardinalitäts-Constraints nachweislich dieselbe Regel befolgen.
#[test]
fn at_most_already_kept_the_contract() {
    let scope = vars(3);
    let constraint = AtMost::new(1, scope.clone(), 1);

    assert!(constraint.is_satisfied(&assign(&[])));
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1)])));
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1)])));
}

/// `violations()` und `is_satisfied()` dürfen nicht auseinanderlaufen — der harte Score ist
/// die Summe der einen, und `is_feasible()` heißt `hard == 0`.
#[test]
fn the_count_agrees_with_satisfaction_under_partial_assignments() {
    let scope = vars(3);
    let cases: Vec<Box<dyn Constraint>> = vec![
        Box::new(ExactlyOne::new(scope.clone(), 1)),
        Box::new(AtLeast::new(2, scope.clone(), 1)),
        Box::new(AtMost::new(1, scope.clone(), 1)),
    ];

    // Jede Teilbelegung über {nicht belegt, 0, 1} für drei Variablen.
    for constraint in &cases {
        for a in [None, Some(0), Some(1)] {
            for b in [None, Some(0), Some(1)] {
                for c in [None, Some(0), Some(1)] {
                    let mut assignment = HashMap::new();
                    for (var, value) in scope.iter().zip([a, b, c]) {
                        if let Some(value) = value {
                            assignment.insert(*var, value);
                        }
                    }
                    assert_eq!(
                        constraint.violations(&assignment) == 0,
                        constraint.is_satisfied(&assignment),
                        "{}: {assignment:?}",
                        constraint.name(),
                    );
                }
            }
        }
    }
}
