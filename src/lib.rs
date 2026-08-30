//! `unifier` — constraint satisfaction / optimization (CSP/COP) modeling
//! and solver framework for Rust, built on [`pathwise`](https://github.com/casoon/pathwise).
//!
//! Concept and scope: see `README.md`. Implementation plan: `plan/`.
//!
//! Layered architecture:
//!
//! ```text
//! model        — variable, domain, interval, activity, resource, group
//! constraint   — equal, not_equal, all_different, no_overlap, cumulative
//! propagation  — constraint graph, AC-3 propagator engine
//! score        — hard/soft priority scoring, incremental updates
//! solver       — backtracking (MRV), branch_and_bound
//! dsl          — problem-building surface API
//! ```

pub mod constraint;
pub mod dsl;
pub mod model;
pub mod propagation;
pub mod score;
pub mod solver;

pub use constraint::{
    AllDifferent, AllowedValues, AtLeast, AtMost, Constraint, Cumulative, Equal, ExactlyOne,
    ForbiddenValues, LessThanOrEqual, NoOverlap, NotEqual, Precedence, TaskDemand,
};
pub use dsl::ModelBuilder;
pub use model::{Activity, Domain, Group, Interval, Resource, Variable, VariableId};
pub use propagation::{ConstraintGraph, PropagationEngine};
pub use score::{HardSoftScore, ScoreCalculator};
pub use solver::{
    BacktrackingSolver, BranchAndBoundSolver, CancellationToken, LocalSearchSolver, LnsSolver,
    ParallelSolver, SearchStatistics, SolveResult, SolverOptions, UnifierProblemAdapter,
};
