//! The graded hard score (plan 52 in timbra): `violations()` counts *how* broken a constraint is,
//! not merely *that* it is.
//!
//! Two properties are checked exhaustively over small instances rather than sampled, because both
//! are absolute:
//!
//! - **Agreement.** `violations() == 0` exactly when `is_satisfied()` is `true`. A count that
//!   disagreed would silently redefine feasibility, since the hard score is their sum and
//!   `is_feasible()` means `hard == 0`.
//! - **A gradient.** Resolving one collision inside an already-broken constraint lowers the count.
//!   This is the whole purpose: without it, a repair search over a constraint spanning dozens of
//!   variables sees a plateau the size of that scope and has no way down.

use std::collections::HashMap;
use unifier::constraint::no_overlap::TaskInterval;
use unifier::constraint::{
    AllDifferent, BucketBlockPattern, BucketRange, BucketedTask, Constraint, Cumulative, NoOverlap,
    TaskDemand,
};
use unifier::{MaximumBucketLoad, VariableId};

/// Every assignment of `values` to `vars`, in order.
fn every_assignment(
    vars: &[VariableId],
    values: &[i64],
) -> impl Iterator<Item = HashMap<VariableId, i64>> {
    let total = values.len().pow(vars.len() as u32);
    let vars = vars.to_vec();
    let values = values.to_vec();
    (0..total).map(move |mut code| {
        let mut assignment = HashMap::new();
        for &var in &vars {
            assignment.insert(var, values[code % values.len()]);
            code /= values.len();
        }
        assignment
    })
}

fn agrees_with_is_satisfied(constraint: &dyn Constraint, vars: &[VariableId], values: &[i64]) {
    for assignment in every_assignment(vars, values) {
        let violations = constraint.violations(&assignment);
        assert_eq!(
            violations == 0,
            constraint.is_satisfied(&assignment),
            "{} disagrees with itself on {assignment:?}: {violations} violations",
            constraint.name(),
        );
    }
}

fn vars(count: u32) -> Vec<VariableId> {
    (0..count).map(VariableId).collect()
}

#[test]
fn all_different_counts_every_variable_past_the_first_on_a_value() {
    let scope = vars(4);
    let constraint = AllDifferent::new(scope.clone());
    agrees_with_is_satisfied(&constraint, &scope, &[0, 1, 2]);

    // All four on one value: three variables too many, and freeing one is visible.
    let mut assignment: HashMap<VariableId, i64> = scope.iter().map(|&var| (var, 0)).collect();
    assert_eq!(constraint.violations(&assignment), 3);
    assignment.insert(scope[0], 1);
    assert_eq!(constraint.violations(&assignment), 2);
}

#[test]
fn no_overlap_counts_pairs() {
    let scope = vars(3);
    let constraint = NoOverlap::new(
        scope
            .iter()
            .map(|&start| TaskInterval { start, duration: 2 })
            .collect(),
    );
    agrees_with_is_satisfied(&constraint, &scope, &[0, 2, 4, 6]);

    // Three tasks stacked on the same slot: three colliding pairs.
    let mut assignment: HashMap<VariableId, i64> = scope.iter().map(|&var| (var, 0)).collect();
    assert_eq!(constraint.violations(&assignment), 3);
    assignment.insert(scope[0], 10);
    assert_eq!(constraint.violations(&assignment), 1);
}

#[test]
fn cumulative_counts_the_overload_itself() {
    let scope = vars(3);
    let constraint = Cumulative::new(
        scope
            .iter()
            .map(|&start| TaskDemand {
                start,
                duration: 1,
                demand: 1,
            })
            .collect(),
        1,
    );
    agrees_with_is_satisfied(&constraint, &scope, &[0, 1, 2]);

    // Three unit demands at once against a capacity of one: two over.
    let mut assignment: HashMap<VariableId, i64> = scope.iter().map(|&var| (var, 0)).collect();
    assert_eq!(constraint.violations(&assignment), 2);
    assignment.insert(scope[0], 5);
    assert_eq!(constraint.violations(&assignment), 1);
}

#[test]
fn maximum_bucket_load_counts_how_far_over_it_is() {
    let scope = vars(3);
    let ranges = vec![BucketRange::new(0, 4, 0), BucketRange::new(4, 8, 1)];
    let constraint = MaximumBucketLoad::new(
        scope
            .iter()
            .map(|&start| BucketedTask::new(start, 1, 1))
            .collect::<Vec<_>>(),
        ranges,
        1,
    );
    agrees_with_is_satisfied(&constraint, &scope, &[0, 1, 4, 5]);

    // All three in the first bucket against a limit of one: two hours too many.
    let mut assignment: HashMap<VariableId, i64> = scope.iter().map(|&var| (var, 0)).collect();
    assert_eq!(constraint.violations(&assignment), 2);
    assignment.insert(scope[0], 4);
    assert_eq!(constraint.violations(&assignment), 1);
}

#[test]
fn a_block_pattern_counts_how_far_from_an_allowed_shape_it_is() {
    let scope = vars(3);
    let ranges = vec![BucketRange::new(0, 8, 0)];
    // Allowed: one block of three, or one of two plus one of one.
    let constraint = BucketBlockPattern::new(
        scope
            .iter()
            .map(|&start| BucketedTask::new(start, 1, 1))
            .collect::<Vec<_>>(),
        ranges,
        vec![vec![3], vec![2, 1]],
    );
    agrees_with_is_satisfied(&constraint, &scope, &[0, 1, 2, 4, 6]);

    // Three separate single hours — [1, 1, 1] — is one hour away from [2, 1].
    let scattered: HashMap<VariableId, i64> = scope
        .iter()
        .enumerate()
        .map(|(index, &var)| (var, index as i64 * 2))
        .collect();
    assert_eq!(constraint.violations(&scattered), 2);
}
