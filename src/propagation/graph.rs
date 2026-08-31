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
use crate::score::Objective;
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

/// A structural defect detected by [`ConstraintGraph::validate`], independent of any concrete
/// assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// A constraint or objective references a variable never registered via `add_variable`.
    UnknownVariable {
        /// Name of the referencing constraint or objective.
        item: String,
        /// The unregistered variable.
        var: VariableId,
    },
    /// A variable's domain is empty, so it can never be assigned.
    EmptyDomain {
        /// The variable with an empty domain.
        var: VariableId,
    },
    /// `add_variable` was called more than once for the same [`VariableId`]; only the last
    /// registration is retained, silently discarding the earlier one.
    DuplicateVariableId {
        /// The variable ID that was registered more than once.
        var: VariableId,
    },
    /// A constraint's own parameters are self-contradictory (see [`Constraint::validate`]).
    InvalidConstraint {
        /// Name of the offending constraint.
        name: String,
        /// Human-readable reason, as returned by [`Constraint::validate`].
        reason: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::UnknownVariable { item, var } => {
                write!(f, "{item} references unregistered variable {var:?}")
            }
            ModelError::EmptyDomain { var } => write!(f, "variable {var:?} has an empty domain"),
            ModelError::DuplicateVariableId { var } => {
                write!(f, "variable id {var:?} was registered more than once")
            }
            ModelError::InvalidConstraint { name, reason } => write!(f, "{name}: {reason}"),
        }
    }
}

impl std::error::Error for ModelError {}

/// Hypergraph constraint network connecting variables and constraints.
#[derive(Clone, Default)]
pub struct ConstraintGraph {
    variables: HashMap<VariableId, Variable>,
    domains: HashMap<VariableId, Domain>,
    constraints: Vec<Arc<dyn Constraint>>,
    objectives: Vec<Arc<dyn Objective>>,
    var_to_constraints: HashMap<VariableId, Vec<ConstraintId>>,
    duplicate_variable_ids: Vec<VariableId>,
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
    /// Registering the same [`VariableId`] twice overwrites the earlier registration and is
    /// flagged by [`Self::validate`].
    ///
    /// # Complexity
    /// Time: O(1) amortized.
    pub fn add_variable(&mut self, variable: Variable, domain: Domain) {
        let id = variable.id();
        if self.variables.contains_key(&id) {
            self.duplicate_variable_ids.push(id);
        }
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
            self.var_to_constraints.entry(var_id).or_default().push(cid);
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

    /// Registers a soft objective term contributing to the `soft` score.
    ///
    /// # Complexity
    /// Time: O(1) amortized.
    pub fn add_objective(&mut self, objective: Arc<dyn Objective>) {
        self.objectives.push(objective);
    }

    /// Returns all registered objective terms.
    #[inline]
    pub fn objectives(&self) -> &[Arc<dyn Objective>] {
        &self.objectives
    }

    /// Validates the graph's structural well-formedness before solving.
    ///
    /// Rejects: constraints/objectives referencing variables never added via [`Self::add_variable`],
    /// variables with an empty domain, [`VariableId`]s registered more than once, and constraints
    /// whose own parameters are self-contradictory (see [`Constraint::validate`]).
    ///
    /// Does not detect infeasibility arising from the *interaction* of otherwise well-formed
    /// constraints — that is what solving determines.
    ///
    /// # Complexity
    /// Time: O(V + C + O) where V, C, O are the number of variables, constraints, and objectives.
    pub fn validate(&self) -> Result<(), Vec<ModelError>> {
        let mut errors = Vec::new();

        for &var in &self.duplicate_variable_ids {
            errors.push(ModelError::DuplicateVariableId { var });
        }

        for (&var, domain) in &self.domains {
            if domain.is_empty() {
                errors.push(ModelError::EmptyDomain { var });
            }
        }

        for constraint in &self.constraints {
            for &var in constraint.scope() {
                if !self.variables.contains_key(&var) {
                    errors.push(ModelError::UnknownVariable {
                        item: constraint.name().to_string(),
                        var,
                    });
                }
            }
            if let Err(reason) = constraint.validate() {
                errors.push(ModelError::InvalidConstraint {
                    name: constraint.name().to_string(),
                    reason,
                });
            }
        }

        for objective in &self.objectives {
            for &var in objective.scope() {
                if !self.variables.contains_key(&var) {
                    errors.push(ModelError::UnknownVariable {
                        item: objective.name().to_string(),
                        var,
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validates the graph (see [`Self::validate`]) and, on success, consumes it into a
    /// [`ValidatedGraph`] that public solvers accept.
    ///
    /// # Complexity
    /// Time: O(V + C + O), see [`Self::validate`].
    pub fn finalize(self) -> Result<ValidatedGraph, Vec<ModelError>> {
        self.validate()?;
        Ok(ValidatedGraph(self))
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

/// A [`ConstraintGraph`] that has passed [`ConstraintGraph::validate`].
///
/// All public solvers accept this type instead of a bare `ConstraintGraph`, so the
/// well-formedness contract from `validate` (no unknown-variable references, no empty domains,
/// no duplicate variable IDs, no self-contradictory constraint parameters) is enforced as part
/// of the solver contract rather than an opt-in check callers can skip by constructing and
/// mutating a `ConstraintGraph` directly.
///
/// Read access is available via [`std::ops::Deref`] to `ConstraintGraph`. There is deliberately
/// no public mutable access and no public constructor other than [`ConstraintGraph::finalize`] —
/// solvers that need to explore mutated *copies* of the underlying graph during search (LNS
/// sub-problems, per-worker clones in [`crate::solver::ParallelSolver`]) do so via
/// `assume_valid`, which is `pub(crate)`-only: it is only ever applied to structural
/// derivatives of a graph that was already validated at its public entry point, never to an
/// arbitrary caller-constructed graph.
#[derive(Debug, Clone)]
pub struct ValidatedGraph(ConstraintGraph);

impl ValidatedGraph {
    /// Returns a reference to the wrapped, validated constraint graph.
    #[inline]
    pub fn graph(&self) -> &ConstraintGraph {
        &self.0
    }

    /// Wraps `graph` as validated without re-running [`ConstraintGraph::validate`].
    ///
    /// Restricted to the crate: only for solver-internal derivatives (domain narrowing, cloning)
    /// of a graph that was already validated at its public entry point.
    pub(crate) fn assume_valid(graph: ConstraintGraph) -> Self {
        Self(graph)
    }
}

impl std::ops::Deref for ValidatedGraph {
    type Target = ConstraintGraph;

    fn deref(&self) -> &ConstraintGraph {
        &self.0
    }
}
