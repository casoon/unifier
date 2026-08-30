//! Composite activity and group primitives for shared interval grouping.
//!
//! A group binds multiple activities together under a shared interval.

use crate::model::activity::ActivityId;
use crate::model::interval::Interval;
use std::fmt;

/// Unique identifier for a composite activity group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "g{}", self.0)
    }
}

/// A group of activities sharing a common execution interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    id: GroupId,
    name: String,
    shared_interval: Interval,
    member_activities: Vec<ActivityId>,
}

impl Group {
    /// Creates a new group with a shared interval.
    ///
    /// # Complexity
    /// Time & Space: O(1) + string allocation.
    pub fn new(id: GroupId, name: impl Into<String>, shared_interval: Interval) -> Self {
        Self {
            id,
            name: name.into(),
            shared_interval,
            member_activities: Vec::new(),
        }
    }

    /// Adds an activity to the group.
    ///
    /// Time complexity: O(1) amortized.
    pub fn add_member(&mut self, activity_id: ActivityId) {
        self.member_activities.push(activity_id);
    }

    /// Returns the group identifier.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn id(&self) -> GroupId {
        self.id
    }

    /// Returns the group name.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the shared interval.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn shared_interval(&self) -> &Interval {
        &self.shared_interval
    }

    /// Returns the member activity IDs.
    ///
    /// Time complexity: O(1).
    #[inline]
    pub fn member_activities(&self) -> &[ActivityId] {
        &self.member_activities
    }
}
