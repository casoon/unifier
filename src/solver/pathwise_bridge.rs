//! Integration bridge connecting `unifier` constraint models with `pathwise` search primitives.
//!
//! Exposes problem state adapters for generic search and optimization routines in `pathwise`.

use crate::model::variable::VariableId;
use crate::propagation::graph::ConstraintGraph;
use crate::score::{HardSoftScore, ScoreCalculator};
use std::collections::HashMap;

/// Adapter bridging a [`ConstraintGraph`] model with generic search problem traits.
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
        self.score_calculator.calculate_score(self.graph, assignment)
    }

    /// Returns a reference to the underlying constraint graph.
    pub fn graph(&self) -> &'a ConstraintGraph {
        self.graph
    }
}
