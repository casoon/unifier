//! Resource representation for scheduling constraints.
//!
//! A resource has a maximum capacity (e.g. 1 for unary resources such as a room or teacher,
//! or > 1 for cumulative resources such as a team or pool of machines).
//!
//! Reference:
//! - Aggoun, A., & Beldiceanu, N. (1993). *Extending CHIP in order to solve cumulative scheduling problems*.
//!   Mathematical and Computer Modelling, 17(7), 57-73.

use std::fmt;

/// Unique identifier for a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(pub u32);

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// A resource with a defined capacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    id: ResourceId,
    name: String,
    capacity: u32,
}

impl Resource {
    /// Creates a new resource with given id, name, and capacity.
    ///
    /// # Complexity
    /// Time: O(1) or O(N) for name allocation.
    /// Space: O(N) where N is name length.
    pub fn new(id: ResourceId, name: impl Into<String>, capacity: u32) -> Self {
        Self {
            id,
            name: name.into(),
            capacity,
        }
    }

    /// Returns the unique resource identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// Returns the resource name.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the resource capacity.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Returns `true` if this is a unary resource (capacity == 1).
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn is_unary(&self) -> bool {
        self.capacity == 1
    }
}
