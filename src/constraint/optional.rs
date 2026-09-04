//! `Optional` constraint wrapper: reifies an inner constraint behind a presence variable.
//!
//! Models an activity/interval that may simply not happen (e.g. an optional maintenance window,
//! or one branch of a resource alternative — see `plan/12-scheduling-vertical.md`, section 3b/3c).
//!
//! Reference:
//! - Laborie, P. (2003). *IBOCP: A declarative framework for constraint-based scheduling*.
//!   Artificial Intelligence, 146(2), 257-302. (Optional/interval-presence reasoning.)

use crate::constraint::{Assignment, Constraint, Explanation, PropagationResult};
use crate::model::domain::{Domain, TrailedDomains};
use crate::model::variable::VariableId;
use std::collections::HashMap;
use std::sync::Arc;

/// Wraps `inner` so it only applies when `presence` is `1`; when `presence` is `0`, `inner` is
/// treated as trivially satisfied and never propagated.
///
/// `presence` should have domain `{0, 1}` (e.g. via [`crate::dsl::ModelBuilder::new_presence_var`]),
/// though this type itself only ever checks `contains(0)`/`contains(1)`, so any domain shape works.
///
/// # Propagation strength
/// This is the simpler of the two designs sketched in `plan/12-scheduling-vertical.md` (section
/// 3b): `propagate()` delegates to `inner` only once `presence` is *confirmed* `1` (its domain no
/// longer contains `0`); while `presence` is still ambiguous (`{0, 1}`), `inner` is not consulted
/// at all — sound (never wrongly prunes) but weaker than full reified propagation, which could
/// additionally infer `presence = 0` from `inner` being unsatisfiable. That direction is
/// deliberately not implemented here (larger, cross-cutting change — would need every wrapped
/// constraint to report *why* it conflicted, not just that it did).
#[derive(Debug)]
pub struct Optional {
    inner: Arc<dyn Constraint>,
    presence: VariableId,
    name: String,
    scope: Vec<VariableId>,
}

impl Optional {
    /// Wraps `inner` behind `presence`.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N = `inner.scope().len()`, to build the combined scope.
    pub fn new(inner: Arc<dyn Constraint>, presence: VariableId) -> Self {
        let name = format!("Optional<{}>", inner.name());
        let mut scope = Vec::with_capacity(inner.scope().len() + 1);
        scope.push(presence);
        scope.extend_from_slice(inner.scope());
        Self {
            inner,
            presence,
            name,
            scope,
        }
    }

    /// Returns the presence variable gating `inner`.
    ///
    /// Time complexity: O(1).
    pub fn presence(&self) -> VariableId {
        self.presence
    }
}

impl Constraint for Optional {
    fn name(&self) -> &str {
        &self.name
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        match assignment.get(&self.presence) {
            Some(0) => true, // absent: vacuously satisfied
            Some(1) => self.inner.is_satisfied(assignment),
            // `presence` unassigned (or, defensively, some other value): not yet decided which
            // branch applies, so — per this crate's "partial assignment doesn't yet violate"
            // convention — not (yet) a violation.
            _ => true,
        }
    }

    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        match assignment.get(&self.presence) {
            Some(1) => self.inner.explain(assignment),
            _ => None,
        }
    }

    fn is_satisfiable(
        &self,
        domains: &HashMap<VariableId, Domain>,
        assignment: &HashMap<VariableId, i64>,
    ) -> bool {
        match assignment.get(&self.presence) {
            Some(0) => true,
            Some(1) => self.inner.is_satisfiable(domains, assignment),
            _ => {
                // `presence` isn't assigned yet. If it can still become 0, the "absent" branch
                // remains reachable regardless of `inner`'s state — optimistically satisfiable,
                // exactly the domain-sensitive reasoning `Constraint::is_satisfiable`'s own doc
                // comment requires (see the P0 fix this mirrors: a branch must not be pruned
                // merely because it isn't satisfied *yet*). Only once `presence`'s domain no
                // longer contains 0 (forced to 1, just not reflected in `assignment` yet) do we
                // fall through to `inner`'s own check.
                let presence_can_be_absent =
                    domains.get(&self.presence).is_some_and(|d| d.contains(0));
                if presence_can_be_absent {
                    true
                } else {
                    self.inner.is_satisfiable(domains, assignment)
                }
            }
        }
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let Some(presence_domain) = domains.get(&self.presence) else {
            return PropagationResult::Success { changed: false };
        };
        if !presence_domain.contains(1) {
            // Forced absent: `inner` can never apply, nothing to propagate.
            return PropagationResult::Success { changed: false };
        }
        if !presence_domain.contains(0) {
            // Forced present: delegate fully.
            return self.inner.propagate(domains);
        }
        // Still ambiguous: conservatively skip `inner` rather than risk pruning a value that's
        // only inconsistent with the *present* branch (see the propagation-strength note above).
        PropagationResult::Success { changed: false }
    }

    fn validate(&self) -> Result<(), String> {
        self.inner.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::NotEqual;
    use crate::model::domain::Domain;

    fn domains(entries: &[(VariableId, Domain)]) -> HashMap<VariableId, Domain> {
        entries.iter().cloned().collect()
    }

    #[test]
    fn test_propagate_skips_inner_when_forced_absent() {
        let x = VariableId(0);
        let y = VariableId(1);
        let presence = VariableId(2);
        let inner = Arc::new(NotEqual::new(x, y));
        let optional = Optional::new(inner, presence);

        let mut trailed = TrailedDomains::new(domains(&[
            (x, Domain::from_values([5])),
            (y, Domain::from_values([5])), // would conflict with NotEqual if active
            (presence, Domain::from_values([0])), // forced absent
        ]));

        assert_eq!(
            optional.propagate(&mut trailed),
            PropagationResult::Success { changed: false },
            "absent: inner's conflict must not surface"
        );
    }

    #[test]
    fn test_propagate_delegates_to_inner_when_forced_present() {
        let x = VariableId(0);
        let y = VariableId(1);
        let presence = VariableId(2);
        let inner = Arc::new(NotEqual::new(x, y));
        let optional = Optional::new(inner, presence);

        let mut trailed = TrailedDomains::new(domains(&[
            (x, Domain::from_values([5])),
            (y, Domain::from_values([5])),
            (presence, Domain::from_values([1])), // forced present
        ]));

        assert_eq!(
            optional.propagate(&mut trailed),
            PropagationResult::Conflict,
            "present: inner's own conflict (x == y, but NotEqual) must surface"
        );
    }

    #[test]
    fn test_propagate_does_not_prune_while_presence_ambiguous() {
        let x = VariableId(0);
        let y = VariableId(1);
        let presence = VariableId(2);
        let inner = Arc::new(NotEqual::new(x, y));
        let optional = Optional::new(inner, presence);

        let mut trailed = TrailedDomains::new(domains(&[
            (x, Domain::from_values([5])),
            (y, Domain::from_values([5])),
            (presence, Domain::range(0, 1)), // still undecided
        ]));

        assert_eq!(
            optional.propagate(&mut trailed),
            PropagationResult::Success { changed: false },
            "ambiguous presence: must not surface inner's conflict yet (would wrongly force \
             presence = 0 without ever having checked whether that's actually required)"
        );
    }

    #[test]
    fn test_is_satisfiable_stays_optimistic_while_presence_can_still_be_absent() {
        let x = VariableId(0);
        let y = VariableId(1);
        let presence = VariableId(2);
        let inner = Arc::new(NotEqual::new(x, y));
        let optional = Optional::new(inner, presence);

        let assignment: HashMap<VariableId, i64> = [(x, 5), (y, 5)].into_iter().collect();
        let domains_map = domains(&[(presence, Domain::range(0, 1))]);

        assert!(
            optional.is_satisfiable(&domains_map, &assignment),
            "presence can still resolve to 0 (absent), so this branch isn't provably dead yet"
        );
    }
}
