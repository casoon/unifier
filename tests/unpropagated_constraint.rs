//! A constraint without a propagator must not cost the tree search the entire subtree.
//!
//! For a long time `BacktrackingSolver` only asked at the leaf whether an assignment is
//! feasible. As long as every constraint propagates, that goes unnoticed: a contradiction
//! shows up as a conflict in the node that causes it. A constraint whose `propagate` says
//! nothing, by contrast, only reports once *everything* is assigned — and if a variable
//! with a large domain lies below the decision that violates it, every single value is
//! tried before the search returns to the cause.
//!
//! That is exactly what happened in `schedulr` (whose `SelectedResourceCapacity` never
//! propagated): 1,116,987 nodes for a single activity. `BranchAndBoundSolver` never had
//! the problem, because its bound via `optimistic_score` asks every constraint for
//! `is_satisfiable`. The tree search now asks the same, at every node, for the constraints
//! of the variable just assigned.

use std::collections::HashMap;
use std::sync::Arc;
use unifier::constraint::PropagationResult;
use unifier::{
    BacktrackingSolver, Constraint, ModelBuilder, SolveStatus, SolverOptions, TrailedDomains,
    VariableId,
};

/// Exactly one of the two variables is 1 — checked, but never propagated.
///
/// The value order tries 0 first, so the search starts with `a = 0, b = 0`: violated as
/// soon as both are assigned, but `propagate` does not say so.
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
            // Partial assignment: not violated yet while something is missing — the contract
            // from `tests/partial_assignment.rs`.
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
    // Large enough that one pass per rejected decision is noticeable; free of constraints,
    // so there would be a solution for each of its values as soon as a and b are right.
    let _wide = builder.new_var("wide", 0..=20_000);
    builder.add_constraint(Arc::new(ExactlyOneSilently { scope: vec![a, b] }));
    let graph = builder.build().expect("valid model");

    let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());

    assert_eq!(outcome.status, SolveStatus::Feasible);
    let solution = outcome.solution.expect("a feasible outcome has a solution");
    assert_eq!(solution.assignment[&a] + solution.assignment[&b], 1);
    // Before: the search assigns a = 0, b = 0, then `wide` — and tries all 20,001 values,
    // each down to the leaf, before it attempts b = 1. The bound is far above what a search
    // needs that sees the contradiction at the node, and far below what it needs without
    // that.
    assert!(
        outcome.statistics.nodes_expanded < 100,
        "{} nodes — the contradiction was only noticed at the leaf",
        outcome.statistics.nodes_expanded
    );
}
