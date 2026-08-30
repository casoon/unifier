//! Activity primitive combining time interval and resource demands.
//!
//! An activity represents a scheduled task consuming resources over a time interval.
//!
//! Reference:
//! - Laborie, P. (2003). *IBOCP: A declarative framework for constraint-based scheduling*.
//!   Artificial Intelligence, 146(2), 257-302.

use crate::model::interval::Interval;
use crate::model::resource::ResourceId;
use std::fmt;

/// Unique identifier for an activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActivityId(pub u32);

impl fmt::Display for ActivityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a{}", self.0)
    }
}

/// Resource demand for an activity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceDemand {
    pub resource_id: ResourceId,
    pub demand: u32,
}

/// An activity in a scheduling model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    id: ActivityId,
    name: String,
    interval: Interval,
    demands: Vec<ResourceDemand>,
}

impl Activity {
    /// Creates a new activity with id, name, interval, and initial demands.
    ///
    /// # Complexity
    /// Time & Space: O(1) or O(N) for string / demands allocation.
    pub fn new(id: ActivityId, name: impl Into<String>, interval: Interval) -> Self {
        Self {
            id,
            name: name.into(),
            interval,
            demands: Vec::new(),
        }
    }

    /// Adds a resource demand requirement to this activity.
    ///
    /// Time complexity: O(1) amortized.
    pub fn require_resource(&mut self, resource_id: ResourceId, demand: u32) {
        self.demands.push(ResourceDemand { resource_id, demand });
    }

    /// Returns the activity identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn id(&self) -> ActivityId {
        self.id
    }

    /// Returns the activity name.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the activity's interval.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn interval(&self) -> &Interval {
        &self.interval
    }

    /// Returns the resource demands.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn demands(&self) -> &[ResourceDemand] {
        &self.demands
    }
}
