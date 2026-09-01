//! Finite domain representations for decision variables.
//!
//! Provides multiple domain representations tailored for different value distributions
//! and solver operations (bound pruning vs. sparse set backtracking).
//!
//! References:
//! - Schulte, C., & Stuckey, P. J. (2008). *Efficient constraint propagation engines*. ACM TOPLAS, 31(1), 1-43.
//! - Briggs, P., & Torczon, L. (1993). *An efficient architecture for sparse sets*. ACM SIGPLAN Notices, 28(3), 115-121.

use crate::model::variable::VariableId;
use std::collections::{BTreeSet, HashMap};

/// Representation of possible integer values for a decision variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Domain {
    /// Range-bounded domain `[min, max]`.
    /// Space complexity: O(1).
    Range { min: i64, max: i64 },

    /// Explicit set of discrete values stored as a sorted BTreeSet or Sparse Set representation.
    /// Space complexity: O(D) where D is domain size.
    Explicit(BTreeSet<i64>),
}

impl Domain {
    /// Creates a range domain `[min, max]`.
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn range(min: i64, max: i64) -> Self {
        if min > max {
            Domain::Range { min: 1, max: 0 } // empty range
        } else {
            Domain::Range { min, max }
        }
    }

    /// Creates an explicit domain from an iterator of integer values.
    ///
    /// # Complexity
    /// Time: O(N log N) where N is number of elements.
    /// Space: O(N).
    pub fn from_values<I: IntoIterator<Item = i64>>(values: I) -> Self {
        let set: BTreeSet<i64> = values.into_iter().collect();
        Domain::Explicit(set)
    }

    /// Checks whether the domain is empty.
    ///
    /// # Complexity
    /// Time: O(1).
    pub fn is_empty(&self) -> bool {
        match self {
            Domain::Range { min, max } => min > max,
            Domain::Explicit(set) => set.is_empty(),
        }
    }

    /// Returns the number of values in the domain.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(1) for Explicit.
    pub fn len(&self) -> usize {
        match self {
            Domain::Range { min, max } => {
                if min > max {
                    0
                } else {
                    // Widen to i128 first: `max - min` overflows i64 arithmetic for a range
                    // spanning close to the full i64 domain (e.g. `Domain::range(i64::MIN, i64::MAX)`).
                    let span = (*max as i128) - (*min as i128) + 1;
                    usize::try_from(span).unwrap_or(usize::MAX)
                }
            }
            Domain::Explicit(set) => set.len(),
        }
    }

    /// Checks if a value is contained in the domain.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(log N) for Explicit.
    pub fn contains(&self, val: i64) -> bool {
        match self {
            Domain::Range { min, max } => val >= *min && val <= *max,
            Domain::Explicit(set) => set.contains(&val),
        }
    }

    /// Returns the minimum value in the domain, or `None` if empty.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(log N) for Explicit.
    pub fn min(&self) -> Option<i64> {
        match self {
            Domain::Range { min, max } => {
                if min > max {
                    None
                } else {
                    Some(*min)
                }
            }
            Domain::Explicit(set) => set.iter().next().copied(),
        }
    }

    /// Returns the maximum value in the domain, or `None` if empty.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(log N) for Explicit.
    pub fn max(&self) -> Option<i64> {
        match self {
            Domain::Range { min, max } => {
                if min > max {
                    None
                } else {
                    Some(*max)
                }
            }
            Domain::Explicit(set) => set.iter().next_back().copied(),
        }
    }

    /// Removes a value from the domain. Returns `true` if the domain was modified.
    ///
    /// # Complexity
    /// Time: O(1) / O(N) depending on representation conversion.
    pub fn remove(&mut self, val: i64) -> bool {
        if !self.contains(val) {
            return false;
        }

        match self {
            Domain::Range { min, max } => {
                if val == *min {
                    // `checked_add` guards the single-value domain `{i64::MAX}`: `min + 1` would
                    // overflow, but the domain becoming empty (the `{min: 1, max: 0}` sentinel
                    // used throughout this type) is exactly the correct result here.
                    match min.checked_add(1) {
                        Some(new_min) => *min = new_min,
                        None => (*min, *max) = (1, 0),
                    }
                    true
                } else if val == *max {
                    // Symmetric guard for the single-value domain `{i64::MIN}`.
                    match max.checked_sub(1) {
                        Some(new_max) => *max = new_max,
                        None => (*min, *max) = (1, 0),
                    }
                    true
                } else {
                    // Split range into explicit set
                    let set: BTreeSet<i64> = (*min..=*max).filter(|&v| v != val).collect();
                    *self = Domain::Explicit(set);
                    true
                }
            }
            Domain::Explicit(set) => set.remove(&val),
        }
    }

    /// Prunes values below `min_val`. Returns `true` if modified.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(K log N) for Explicit where K is number of removed elements.
    pub fn remove_below(&mut self, min_val: i64) -> bool {
        match self {
            Domain::Range { min, .. } => {
                if *min < min_val {
                    *min = min_val;
                    true
                } else {
                    false
                }
            }
            Domain::Explicit(set) => {
                let to_remove: Vec<i64> = set.range(..min_val).copied().collect();
                if to_remove.is_empty() {
                    false
                } else {
                    for v in to_remove {
                        set.remove(&v);
                    }
                    true
                }
            }
        }
    }

    /// Prunes values above `max_val`. Returns `true` if modified.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(K log N) for Explicit where K is number of removed elements.
    pub fn remove_above(&mut self, max_val: i64) -> bool {
        match self {
            Domain::Range { min: _, max } => {
                if *max > max_val {
                    *max = max_val;
                    true
                } else {
                    false
                }
            }
            Domain::Explicit(set) => {
                // `max_val + 1` would overflow for `max_val == i64::MAX`; nothing can be "above"
                // it in that case, so there is nothing to remove.
                let to_remove: Vec<i64> = match max_val.checked_add(1) {
                    Some(lower_bound) => set.range(lower_bound..).copied().collect(),
                    None => Vec::new(),
                };

                if to_remove.is_empty() {
                    false
                } else {
                    for v in to_remove {
                        set.remove(&v);
                    }
                    true
                }
            }
        }
    }

    /// Restricts the domain to a single value. Returns `true` if modified.
    ///
    /// # Complexity
    /// Time: O(1) for Range, O(N) for Explicit.
    pub fn assign(&mut self, val: i64) -> bool {
        if !self.contains(val) {
            *self = Domain::Range { min: 1, max: 0 }; // empty
            return true;
        }
        if self.len() == 1 {
            return false;
        }
        *self = Domain::Range { min: val, max: val };
        true
    }

    /// Returns all values in the domain as a vector.
    ///
    /// # Complexity
    /// Time: O(N) where N = domain size.
    /// Space: O(N).
    pub fn values(&self) -> Vec<i64> {
        match self {
            Domain::Range { min, max } => {
                if min > max {
                    Vec::new()
                } else {
                    (*min..=*max).collect()
                }
            }
            Domain::Explicit(set) => set.iter().copied().collect(),
        }
    }
}

/// A `HashMap<VariableId, Domain>` that records the pre-mutation value of every domain accessed
/// mutably, so a search node can be undone in `O(changed)` by restoring only the domains that
/// were actually touched — instead of an `O(N)` full-map clone of all `N` domains at every node.
///
/// Reference:
/// - Schulte, C. (1999). *Comparing trailing and copying for constraint programming*. ICLP 1999,
///   275-289. (Trailing vs. copying as the two classical state-restoration strategies for CSP
///   search; this type implements trailing.)
///
/// Read access is available via [`std::ops::Deref`] to the underlying `HashMap`, so existing
/// read-only call sites (`domains.get(&var)`, `domains.contains_key(&var)`, ...) work unchanged.
/// There is deliberately no `DerefMut`: mutation must go through [`Self::get_mut`], the only way
/// to record an undo entry, so [`Self::undo_to`] can never miss a change.
#[derive(Debug, Clone, Default)]
pub struct TrailedDomains {
    domains: HashMap<VariableId, Domain>,
    trail: Vec<(VariableId, Domain)>,
}

impl TrailedDomains {
    /// Wraps `domains` with an empty trail.
    ///
    /// # Complexity
    /// Time & Space: O(1) (takes ownership; no copy).
    pub fn new(domains: HashMap<VariableId, Domain>) -> Self {
        Self {
            domains,
            trail: Vec::new(),
        }
    }

    /// Returns a mutable reference to `var`'s domain, first recording its current value on the
    /// trail so [`Self::undo_to`] can restore it later. Returns `None` if `var` is untracked.
    ///
    /// Shadows `HashMap::get_mut` (an inherent method takes priority over the `Deref` target's),
    /// so existing `domains.get_mut(&var)` call sites route through here automatically.
    ///
    /// # Complexity
    /// Time: O(1) amortized, plus the cost of cloning the domain being recorded (O(1) for
    /// `Domain::Range`, O(D) for `Domain::Explicit`).
    pub fn get_mut(&mut self, var: &VariableId) -> Option<&mut Domain> {
        if let Some(current) = self.domains.get(var) {
            self.trail.push((*var, current.clone()));
        }
        self.domains.get_mut(var)
    }

    /// Applies `narrow` to `var`'s domain, recording an undo entry only if `narrow` reports (via
    /// its `bool` return) that it actually modified the domain.
    ///
    /// Prefer this over [`Self::get_mut`] when a call site probes many variables per propagation
    /// step but expects only a few to actually change (e.g. a newly-fixed value pruned from
    /// every other variable in a wide `AllDifferent` scope, most of which don't contain it
    /// anyway): `get_mut` would record — and later have to undo — an entry for every probed
    /// variable regardless of whether anything changed.
    ///
    /// Returns `None` if `var` is untracked, `Some(changed)` otherwise.
    ///
    /// # Complexity
    /// Time: O(1) amortized, plus the cost of cloning the domain being probed (O(1) for
    /// `Domain::Range`, O(D) for `Domain::Explicit`) — paid once per call regardless of whether
    /// `narrow` reports a change, since the pre-mutation value must be captured before `narrow`
    /// runs.
    pub fn mutate(
        &mut self,
        var: VariableId,
        narrow: impl FnOnce(&mut Domain) -> bool,
    ) -> Option<bool> {
        let before = self.domains.get(&var)?.clone();
        let changed = narrow(self.domains.get_mut(&var)?);
        if changed {
            self.trail.push((var, before));
        }
        Some(changed)
    }

    /// Returns a checkpoint identifying the current trail position, to later pass to
    /// [`Self::undo_to`].
    ///
    /// # Complexity
    /// Time & Space: O(1).
    pub fn checkpoint(&self) -> usize {
        self.trail.len()
    }

    /// Restores every domain mutated since `checkpoint`, in reverse order, back to its
    /// pre-mutation value.
    ///
    /// # Complexity
    /// Time: O(K) where K is the number of mutations recorded since `checkpoint` (not O(N)).
    pub fn undo_to(&mut self, checkpoint: usize) {
        while self.trail.len() > checkpoint {
            let (var, previous) = self
                .trail
                .pop()
                .expect("trail.len() > checkpoint implies non-empty");
            self.domains.insert(var, previous);
        }
    }

    /// Returns the distinct variables with a trail entry recorded since `checkpoint`, i.e. the
    /// variables [`Self::get_mut`]/[`Self::mutate`] touched. Used by the propagation engine's
    /// event-based re-enqueueing (see `plan/11-search-heuristics-and-global-constraints.md`,
    /// part B) to learn exactly which of a constraint's scope variables it modified during a
    /// `propagate()` call — reusing the trail already recorded for undo instead of re-deriving
    /// the same information via extra domain lookups.
    ///
    /// May over-report a variable whose net effect was a no-op if a caller used
    /// [`Self::get_mut`] (which records unconditionally) rather than [`Self::mutate`] (which
    /// only records on a confirmed change) — callers that need exactness should prefer `mutate`.
    ///
    /// # Panics
    /// If `checkpoint` exceeds the current trail length. Valid checkpoints are values previously
    /// returned by [`Self::checkpoint`] on this same instance — as with [`Self::undo_to`], a
    /// checkpoint from before an intervening `undo_to` call is no longer valid.
    ///
    /// # Complexity
    /// Time: O(K) where K = trail entries since `checkpoint`. Space: O(distinct variables
    /// touched).
    pub fn changed_since(&self, checkpoint: usize) -> impl Iterator<Item = VariableId> + '_ {
        let mut seen: Vec<VariableId> = Vec::new();
        self.trail[checkpoint..].iter().filter_map(move |(var, _)| {
            if seen.contains(var) {
                None
            } else {
                seen.push(*var);
                Some(*var)
            }
        })
    }
}

impl std::ops::Deref for TrailedDomains {
    type Target = HashMap<VariableId, Domain>;

    fn deref(&self) -> &HashMap<VariableId, Domain> {
        &self.domains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_domain_basic() {
        let mut d = Domain::range(1, 5);
        assert_eq!(d.len(), 5);
        assert_eq!(d.min(), Some(1));
        assert_eq!(d.max(), Some(5));

        assert!(d.remove(1));
        assert_eq!(d.min(), Some(2));
        assert_eq!(d.len(), 4);

        assert!(d.remove(3)); // Middle element splits into explicit
        assert_eq!(d.values(), vec![2, 4, 5]);
    }

    #[test]
    fn test_explicit_domain_pruning() {
        let mut d = Domain::from_values(vec![10, 20, 30, 40]);
        assert_eq!(d.len(), 4);
        assert!(d.remove_below(20));
        assert_eq!(d.values(), vec![20, 30, 40]);
        assert!(d.remove_above(30));
        assert_eq!(d.values(), vec![20, 30]);
    }

    #[test]
    fn test_len_near_i64_boundaries_does_not_overflow() {
        // A moderate range anchored at the i64 boundary: realistic if a modeler uses i64::MAX as
        // a sentinel "unbounded" upper bound.
        let d = Domain::range(i64::MAX - 9, i64::MAX);
        assert_eq!(d.len(), 10);
        assert_eq!(d.min(), Some(i64::MAX - 9));
        assert_eq!(d.max(), Some(i64::MAX));

        // The extreme case: `max - min` alone overflows i64 arithmetic.
        let full = Domain::range(i64::MIN, i64::MAX);
        assert_eq!(full.len(), usize::MAX);
        assert!(!full.is_empty());
    }

    #[test]
    fn test_remove_single_value_domain_at_i64_extremes_does_not_overflow() {
        // Removing the only value of a domain pinned at i64::MAX: `min + 1` would overflow.
        let mut at_max = Domain::range(i64::MAX, i64::MAX);
        assert!(at_max.remove(i64::MAX));
        assert!(at_max.is_empty());

        // Symmetric case: `max - 1` would overflow.
        let mut at_min = Domain::range(i64::MIN, i64::MIN);
        assert!(at_min.remove(i64::MIN));
        assert!(at_min.is_empty());
    }

    #[test]
    fn test_remove_above_i64_max_on_explicit_domain_does_not_overflow() {
        // `max_val + 1` would overflow for `max_val == i64::MAX`; nothing is above it, so
        // nothing should be removed.
        let mut d = Domain::from_values(vec![1, 2, i64::MAX]);
        assert!(!d.remove_above(i64::MAX));
        assert_eq!(d.values(), vec![1, 2, i64::MAX]);
    }

    #[test]
    fn test_trailed_domains_undo_restores_single_mutation() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let checkpoint = trailed.checkpoint();
        trailed.get_mut(&v).unwrap().remove_above(5);
        assert_eq!(trailed.get(&v).unwrap().max(), Some(5));

        trailed.undo_to(checkpoint);
        assert_eq!(trailed.get(&v).unwrap(), &Domain::range(1, 10));
    }

    #[test]
    fn test_trailed_domains_undo_restores_multiple_mutations_in_order() {
        // Two variables mutated, then a third narrowing on the first: undo must reconstruct the
        // exact original state, not just the state before the last mutation.
        let mut map = HashMap::new();
        let x = VariableId(0);
        let y = VariableId(1);
        map.insert(x, Domain::range(1, 10));
        map.insert(y, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let checkpoint = trailed.checkpoint();
        trailed.get_mut(&x).unwrap().remove_above(8);
        trailed.get_mut(&y).unwrap().remove_below(3);
        trailed.get_mut(&x).unwrap().remove_below(2);
        assert_eq!(trailed.get(&x).unwrap(), &Domain::range(2, 8));
        assert_eq!(trailed.get(&y).unwrap(), &Domain::range(3, 10));

        trailed.undo_to(checkpoint);
        assert_eq!(trailed.get(&x).unwrap(), &Domain::range(1, 10));
        assert_eq!(trailed.get(&y).unwrap(), &Domain::range(1, 10));
    }

    #[test]
    fn test_trailed_domains_nested_checkpoints() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let outer = trailed.checkpoint();
        trailed.get_mut(&v).unwrap().remove_above(8);
        let inner = trailed.checkpoint();
        trailed.get_mut(&v).unwrap().remove_above(5);
        assert_eq!(trailed.get(&v).unwrap().max(), Some(5));

        // Undo only the inner mutation.
        trailed.undo_to(inner);
        assert_eq!(trailed.get(&v).unwrap().max(), Some(8));

        // Undo the rest.
        trailed.undo_to(outer);
        assert_eq!(trailed.get(&v).unwrap(), &Domain::range(1, 10));
    }

    #[test]
    fn test_trailed_domains_deref_read_access() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let trailed = TrailedDomains::new(map);

        // Deref makes read-only HashMap methods available directly.
        assert!(trailed.contains_key(&v));
        assert_eq!(trailed.len(), 1);
        assert_eq!(trailed.get(&v), Some(&Domain::range(1, 10)));
    }

    #[test]
    fn test_trailed_domains_mutate_no_op_does_not_grow_trail() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let checkpoint = trailed.checkpoint();
        // remove_above(20) on a domain already bounded by 10 is a no-op (returns false).
        let changed = trailed.mutate(v, |d| d.remove_above(20));
        assert_eq!(changed, Some(false));
        assert_eq!(
            trailed.checkpoint(),
            checkpoint,
            "no-op mutation must not grow the trail"
        );
        assert_eq!(trailed.get(&v).unwrap(), &Domain::range(1, 10));
    }

    #[test]
    fn test_trailed_domains_mutate_confirmed_change_is_undoable() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let checkpoint = trailed.checkpoint();
        let changed = trailed.mutate(v, |d| d.remove_above(5));
        assert_eq!(changed, Some(true));
        assert_eq!(trailed.get(&v).unwrap().max(), Some(5));

        trailed.undo_to(checkpoint);
        assert_eq!(trailed.get(&v).unwrap(), &Domain::range(1, 10));
    }

    #[test]
    fn test_changed_since_reports_distinct_touched_variables() {
        let mut map = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        map.insert(a, Domain::range(1, 10));
        map.insert(b, Domain::range(1, 10));
        map.insert(c, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let checkpoint = trailed.checkpoint();
        // `c` is untouched; `a` is mutated twice (should be reported once).
        assert_eq!(trailed.mutate(a, |d| d.remove_above(5)), Some(true));
        assert_eq!(trailed.mutate(b, |d| d.remove_above(5)), Some(true));
        assert_eq!(trailed.mutate(a, |d| d.remove_above(3)), Some(true));

        let mut touched: Vec<VariableId> = trailed.changed_since(checkpoint).collect();
        touched.sort_by_key(|v| v.0);
        assert_eq!(touched, vec![a, b]);
    }

    #[test]
    fn test_changed_since_empty_when_nothing_mutated_after_checkpoint() {
        let mut map = HashMap::new();
        let v = VariableId(0);
        map.insert(v, Domain::range(1, 10));
        let mut trailed = TrailedDomains::new(map);

        let _ = trailed.mutate(v, |d| d.remove_above(5));
        let checkpoint = trailed.checkpoint();
        assert_eq!(trailed.mutate(v, |d| d.remove_above(5)), Some(false)); // no-op, no trail entry

        assert_eq!(trailed.changed_since(checkpoint).count(), 0);
    }
}
