//! Compact periodic domain restriction for calendar-shaped integer variables.

use crate::constraint::{Assignment, Constraint, Explanation, PropagationResult};
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use std::collections::{BTreeSet, HashMap};

/// Restricts a variable to allowed offsets in a repeating integer period, with absolute
/// unavailable exception ranges. The stored representation is O(P + E), independent of the
/// modeled horizon.
#[derive(Debug, Clone)]
pub struct PeriodicValues {
    var: VariableId,
    period: i64,
    allowed_offsets: BTreeSet<i64>,
    unavailable_ranges: Vec<(i64, i64)>,
    scope: [VariableId; 1],
}

impl PeriodicValues {
    pub fn new(
        var: VariableId,
        period: i64,
        allowed_offsets: impl IntoIterator<Item = i64>,
        unavailable_ranges: impl IntoIterator<Item = (i64, i64)>,
    ) -> Self {
        Self {
            var,
            period,
            allowed_offsets: allowed_offsets.into_iter().collect(),
            unavailable_ranges: unavailable_ranges.into_iter().collect(),
            scope: [var],
        }
    }

    pub fn is_allowed(&self, value: i64) -> bool {
        !self
            .unavailable_ranges
            .iter()
            .any(|&(start, end)| start <= value && value <= end)
            && self.period > 0
            && self
                .allowed_offsets
                .contains(&value.rem_euclid(self.period))
    }
}

impl Constraint for PeriodicValues {
    fn name(&self) -> &str {
        "PeriodicValues"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        assignment
            .get(&self.var)
            .is_none_or(|&value| self.is_allowed(value))
    }

    fn explain(&self, assignment: &Assignment) -> Option<Explanation> {
        let &value = assignment.get(&self.var)?;
        (!self.is_allowed(value)).then(|| Explanation {
            constraint_name: "PeriodicValues",
            involved: vec![self.var],
            message: format!("value {value} is unavailable in the periodic calendar"),
        })
    }

    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let Some(domain) = domains.get_mut(&self.var) else {
            return PropagationResult::Success { changed: false };
        };
        let mut changed = false;
        if domain.len() <= 4096 {
            for value in domain.values() {
                if !self.is_allowed(value) && domain.remove(value) {
                    changed = true;
                }
            }
        } else {
            if let Some(min) = domain.min()
                && let Some(next) = self.next_allowed(min)
            {
                changed |= domain.remove_below(next);
            }
            if let Some(max) = domain.max()
                && let Some(previous) = self.previous_allowed(max)
            {
                changed |= domain.remove_above(previous);
            }
        }
        if domain.is_empty() {
            PropagationResult::Conflict
        } else {
            PropagationResult::Success { changed }
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.period <= 0 {
            return Err("period must be positive".to_string());
        }
        if self.allowed_offsets.is_empty() {
            return Err("at least one allowed offset is required".to_string());
        }
        if self
            .allowed_offsets
            .iter()
            .any(|&offset| offset < 0 || offset >= self.period)
        {
            return Err("allowed offsets must be within 0..period".to_string());
        }
        if self
            .unavailable_ranges
            .iter()
            .any(|&(start, end)| start > end)
        {
            return Err("unavailable range start must not exceed its end".to_string());
        }
        Ok(())
    }
}

impl PeriodicValues {
    fn next_allowed(&self, mut value: i64) -> Option<i64> {
        loop {
            if let Some(&(_, end)) = self
                .unavailable_ranges
                .iter()
                .find(|&&(start, end)| start <= value && value <= end)
            {
                value = end.checked_add(1)?;
                continue;
            }
            for _ in 0..self.period {
                if self.is_allowed(value) {
                    return Some(value);
                }
                value = value.checked_add(1)?;
            }
        }
    }

    fn previous_allowed(&self, mut value: i64) -> Option<i64> {
        loop {
            if let Some(&(start, _)) = self
                .unavailable_ranges
                .iter()
                .find(|&&(start, end)| start <= value && value <= end)
            {
                value = start.checked_sub(1)?;
                continue;
            }
            for _ in 0..self.period {
                if self.is_allowed(value) {
                    return Some(value);
                }
                value = value.checked_sub(1)?;
            }
        }
    }
}
