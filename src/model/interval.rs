//! Interval primitives for time-based scheduling and resource modeling.
//!
//! An interval models a task or activity with `start`, `duration`, and `end` variables
//! satisfying the implicit constraint `end = start + duration`.
//!
//! Reference:
//! - Baptiste, P., Le Pape, C., & Nuijten, W. (2001). *Constraint-Based Scheduling*. Springer.

use crate::model::variable::VariableId;

/// Duration specification for an [`Interval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationSpec {
    /// Fixed integer duration.
    Fixed(u64),
    /// Variable duration bound to a decision variable.
    Variable(VariableId),
}

/// An interval decision variable tuple representing `[start, start + duration)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Interval {
    start: VariableId,
    duration: DurationSpec,
    end: VariableId,
}

impl Interval {
    /// Creates a new interval with start, duration, and end variables.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new(start: VariableId, duration: DurationSpec, end: VariableId) -> Self {
        Self {
            start,
            duration,
            end,
        }
    }

    /// Returns the start variable identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn start(&self) -> VariableId {
        self.start
    }

    /// Returns the duration specification.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn duration(&self) -> DurationSpec {
        self.duration
    }

    /// Returns the end variable identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn end(&self) -> VariableId {
        self.end
    }
}
