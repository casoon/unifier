//! Integration bridge connecting `unifier` constraint models with `pathwise` search primitives.
//!
//! Exposes problem state adapters for generic search and optimization routines in `pathwise`.
//!
//! Reference:
//! - `pathwise::core::problem::{Problem, OptimizationProblem}`.

use crate::model::variable::VariableId;
use crate::propagation::graph::ConstraintGraph;
use crate::score::{HardSoftScore, ScoreCalculator};
use pathwise::core::problem::{OptimizationProblem, Problem};
use std::collections::HashMap;

/// A single-variable assignment move: assign `1` (`VariableId`) the value `1` (`i64`).
pub type Move = (VariableId, i64);

/// Adapter bridging a [`ConstraintGraph`] model with generic search problem traits.
///
/// Implements `pathwise`'s [`Problem`]/[`OptimizationProblem`] traits so `unifier` models can be
/// driven by `pathwise`'s generic search and optimization algorithms (Hill Climbing, Tabu Search,
/// Simulated Annealing, ...), not just `unifier`'s own CSP-specialized solvers.
///
/// A [`Problem::State`] is a (possibly partial) variable assignment. A [`Problem::Move`] assigns
/// one still-unassigned variable (the lowest [`VariableId`] not yet in the state, mirroring
/// `select_mrv_variable`'s "process variables in a fixed order" contract, though
/// without domain-size ordering since `pathwise` algorithms explore moves generically) to one
/// value from its *original* graph domain — this adapter does not run `unifier`'s AC-3
/// propagation, so `pathwise`-driven search sees a weaker (unpruned) branching factor than
/// [`crate::solver::BacktrackingSolver`].
#[derive(Debug, Clone)]
pub struct UnifierProblemAdapter<'a> {
    graph: &'a ConstraintGraph,
    score_calculator: ScoreCalculator,
}

impl<'a> UnifierProblemAdapter<'a> {
    /// Creates a pathwise search adapter for the given constraint graph.
    pub fn new(graph: &'a ConstraintGraph) -> Self {
        Self {
            graph,
            score_calculator: ScoreCalculator,
        }
    }

    /// Evaluates the hard/soft score for a candidate assignment state.
    ///
    /// Time complexity: O(C) where C is number of constraints.
    pub fn score(&self, assignment: &HashMap<VariableId, i64>) -> HardSoftScore {
        self.score_calculator
            .calculate_score(self.graph, assignment)
    }

    /// Returns a reference to the underlying constraint graph.
    pub fn graph(&self) -> &'a ConstraintGraph {
        self.graph
    }

    /// Returns the lowest-ID variable not yet present in `state`, or `None` if `state` is
    /// already a complete assignment.
    fn next_unassigned(&self, state: &HashMap<VariableId, i64>) -> Option<VariableId> {
        self.graph
            .variables()
            .keys()
            .copied()
            .filter(|v| !state.contains_key(v))
            .min()
    }
}

impl<'a> Problem for UnifierProblemAdapter<'a> {
    type State = HashMap<VariableId, i64>;
    type Move = Move;

    /// Time & Space: O(1).
    fn initial(&self) -> Self::State {
        HashMap::new()
    }

    /// Time: O(D) where D is the chosen variable's original domain size.
    fn moves(&self, state: &Self::State) -> impl Iterator<Item = Self::Move> {
        self.next_unassigned(state)
            .into_iter()
            .flat_map(move |var| {
                let values = self
                    .graph
                    .domains()
                    .get(&var)
                    .map(|d| d.values())
                    .unwrap_or_default();
                values.into_iter().map(move |val| (var, val))
            })
    }

    /// Time & Space: O(N) where N is `state.len()` (clone + insert).
    fn apply(&self, state: &Self::State, mv: &Self::Move) -> Self::State {
        let mut next = state.clone();
        next.insert(mv.0, mv.1);
        next
    }

    /// Time: O(C) where C is number of constraints (delegates to [`Self::score`]).
    fn is_goal(&self, state: &Self::State) -> bool {
        state.len() == self.graph.variables().len() && self.score(state).is_feasible()
    }
}

impl<'a> OptimizationProblem for UnifierProblemAdapter<'a> {
    type Score = HardSoftScore;

    /// Time: O(C) where C is number of constraints.
    fn score(&self, state: &Self::State) -> Self::Score {
        UnifierProblemAdapter::score(self, state)
    }
}
