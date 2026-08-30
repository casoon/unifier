//! Decision variable primitives for Constraint Satisfaction Problems (CSP).
//!
//! Reference:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann. Chapter 2: Constraint Networks.
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.

use std::fmt;

/// Unique identifier for a decision variable within a constraint model.
///
/// Space complexity: O(1) space (32-bit integer identifier).
/// Time complexity: O(1) copy/comparison operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariableId(pub u32);

impl fmt::Display for VariableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// A decision variable in a CSP/COP model.
///
/// Each variable has a unique [`VariableId`] and a human-readable name for debugging
/// and DSL inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    id: VariableId,
    name: String,
}

impl Variable {
    /// Creates a new decision variable with the given identifier and name.
    ///
    /// # Complexity
    /// Time: O(1) if `name` is moved, or O(N) string allocation where N = `name.len()`.
    /// Space: O(N) where N is name string length.
    pub fn new(id: VariableId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    /// Returns the unique variable identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn id(&self) -> VariableId {
        self.id
    }

    /// Returns the human-readable variable name.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }
}
