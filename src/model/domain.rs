//! Finite domain representations for decision variables.
//!
//! Provides multiple domain representations tailored for different value distributions
//! and solver operations (bound pruning vs. sparse set backtracking).
//!
//! References:
//! - Schulte, C., & Stuckey, P. J. (2008). *Efficient constraint propagation engines*. ACM TOPLAS, 31(1), 1-43.
//! - Briggs, P., & Torczon, L. (1993). *An efficient architecture for sparse sets*. ACM SIGPLAN Notices, 28(3), 115-121.

use std::collections::BTreeSet;

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
}
