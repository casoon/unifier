//! Constraint hypergraph network and AC-3 propagation engine.

pub mod engine;
pub mod graph;

pub use engine::{ConstraintWeights, PropagationEngine};
pub use graph::{ConstraintGraph, ConstraintId, ConstraintViolation, ModelError, ValidatedGraph};
