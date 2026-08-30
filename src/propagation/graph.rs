//! Hypergraph constraint network representation.
//!
//! Maintains variables, initial domains, registered constraints, and adjacency indexes
//! connecting variables to constraints for incremental propagation.
//!
//! References:
//! - Dechter, R. (2003). *Constraint Processing*. Morgan Kaufmann. Chapter 2: Constraint Networks.
//! - Rossi, F., van Beek, P., & Walsh, T. (2006). *Handbook of Constraint Programming*. Elsevier.

use crate::constraint::Constraint;
use crate::model::domain::Domain;
use crate::model::variable::{Variable, VariableId};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Unique identifier for a constraint registered in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub u32);

impl fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// Hypergraph constraint network connecting variables and constraints.
#[derive(Clone, Default)]
pub struct ConstraintGraph {
    variables: HashMap<VariableId, Variable>,
    domains: HashMap<VariableId, Domain>,
    constraints: Vec<Arc<dyn Constraint>>,
    var_to_constraints: HashMap<VariableId, Vec<ConstraintId>>,
}

impl ConstraintGraph {
    /// Creates a new empty constraint graph.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a variable with an initial domain to the graph.
    ///
    /// # Complexity
    /// Time: O(1) amortized.
    pub fn add_variable(&mut self, variable: Variable, domain: Domain) {
        let id = variable.id();
        self.variables.insert(id, variable);
        self.domains.insert(id, domain);
        self.var_to_constraints.entry(id).or_default();
    }

    /// Adds a constraint to the graph and indexes its scope.
    ///
    /// # Complexity
    /// Time: O(K) where K is number of variables in constraint scope.
    pub fn add_constraint(&mut self, constraint: Arc<dyn Constraint>) -> ConstraintId {
        let cid = ConstraintId(self.constraints.len() as u32);

        for &var_id in constraint.scope() {
            self.var_to_constraints
                .entry(var_id)
                .or_default()
                .push(cid);
        }

        self.constraints.push(constraint);
        cid
    }

    /// Returns a reference to the variable map.
    #[inline]
    pub fn variables(&self) -> &HashMap<VariableId, Variable> {
        &self.variables
    }

    /// Returns a reference to the domain map.
    #[inline]
    pub fn domains(&self) -> &HashMap<VariableId, Domain> {
        &self.domains
    }

    /// Returns a mutable reference to the domain map.
    #[inline]
    pub fn domains_mut(&mut self) -> &mut HashMap<VariableId, Domain> {
        &mut self.domains
    }

    /// Returns all registered constraints.
    #[inline]
    pub fn constraints(&self) -> &[Arc<dyn Constraint>] {
        &self.constraints
    }

    /// Returns constraint IDs registered for a specific variable.
    pub fn constraints_for_variable(&self, var_id: VariableId) -> &[ConstraintId] {
        self.var_to_constraints
            .get(&var_id)
            .map(|vec| vec.as_slice())
            .unwrap_or(&[])
    }

    /// Returns a constraint by ID.
    pub fn get_constraint(&self, id: ConstraintId) -> Option<&Arc<dyn Constraint>> {
        self.constraints.get(id.0 as usize)
    }
}

impl fmt::Debug for ConstraintGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConstraintGraph")
            .field("num_variables", &self.variables.len())
            .field("num_constraints", &self.constraints.len())
            .finish()
    }
}
