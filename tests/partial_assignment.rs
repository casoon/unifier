//! The contract that `Constraint::is_satisfied` promises under a **partial** assignment:
//! `false` only if what is already assigned violates the constraint — not merely because
//! something is still missing.
//!
//! That is how the method's documentation states it ("or if unassigned variables do not violate
//! the constraint yet"), and most constraints uphold it: `AllDifferent` only looks at the
//! assigned variables, `Equal` only compares once both sides are present, `AtMost` counts
//! upwards. `ExactlyOne` (`== 1`) and `AtLeast` (`>= k`) did not — they reported a group as
//! violated in which nobody had taken the target yet, even though every variable in it could
//! still become it.
//!
//! Why this is not cosmetic: the greedy construction in `LocalSearchSolver` only sets a variable
//! without retraction when *no* constraint is violated. A variable in a still untouched choice
//! group could never meet that condition and therefore ran into retraction — along with the
//! variables it took with it. Measured in plan/58, K3.
//!
//! On a **complete** assignment the contract changes nothing, and that is exactly what the tests
//! here check as well: there nothing is unassigned, and both constraints fall back to their old
//! condition. The hard score, which sums over complete assignments, sees the same numbers as
//! before.

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

    // Nothing assigned: any of the three can still become the one.
    assert!(constraint.is_satisfied(&assign(&[])));

    // One zero assigned, two open: still reachable.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0)])));

    // The one is in place — satisfied, whatever comes next.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1)])));

    // Two are on the target: that will not come down again, so violated, even when partial.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1)])));

    // Fully assigned and none on the target: violated. Here the contract falls back to the
    // old condition, and the hard score counts as before.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 0), (scope[2], 0)])));
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 1), (scope[2], 0)])));
}

#[test]
fn at_least_waits_until_the_count_is_out_of_reach() {
    let scope = vars(3);
    let constraint = AtLeast::new(2, scope.clone(), 1);

    assert!(constraint.is_satisfied(&assign(&[])));

    // One off target, two open — two are still reachable.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 0)])));

    // Two off target, one open: two are no longer reachable.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 0), (scope[1], 0)])));

    // Complete, two on the target: satisfied.
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1), (scope[2], 0)])));
    // Complete, one on the target: violated.
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 0), (scope[2], 0)])));
}

/// `AtMost` always kept the contract — included here as a cross-check, so the three
/// cardinality constraints demonstrably follow the same rule.
#[test]
fn at_most_already_kept_the_contract() {
    let scope = vars(3);
    let constraint = AtMost::new(1, scope.clone(), 1);

    assert!(constraint.is_satisfied(&assign(&[])));
    assert!(constraint.is_satisfied(&assign(&[(scope[0], 1)])));
    assert!(!constraint.is_satisfied(&assign(&[(scope[0], 1), (scope[1], 1)])));
}

/// `violations()` and `is_satisfied()` must not diverge — the hard score is the sum of the
/// former, and `is_feasible()` means `hard == 0`.
#[test]
fn the_count_agrees_with_satisfaction_under_partial_assignments() {
    let scope = vars(3);
    let cases: Vec<Box<dyn Constraint>> = vec![
        Box::new(ExactlyOne::new(scope.clone(), 1)),
        Box::new(AtLeast::new(2, scope.clone(), 1)),
        Box::new(AtMost::new(1, scope.clone(), 1)),
    ];

    // Every partial assignment over {unassigned, 0, 1} for three variables.
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
