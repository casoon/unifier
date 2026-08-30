//! Domain modeling components for variables, domains, intervals, resources, activities, and groups.

pub mod activity;
pub mod domain;
pub mod group;
pub mod interval;
pub mod resource;
pub mod variable;

pub use activity::{Activity, ActivityId, ResourceDemand};
pub use domain::Domain;
pub use group::{Group, GroupId};
pub use interval::{DurationSpec, Interval};
pub use resource::{Resource, ResourceId};
pub use variable::{Variable, VariableId};
