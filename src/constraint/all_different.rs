//! Global `AllDifferent` constraint enforcing pairwise distinction across a set of variables.
//!
//! References:
//! - Régin, J. C. (1994). *A filtering algorithm for constraints of difference in CSPs*. AAAI-94, 362-367.
//! - van Hoeve, W. J. (2001). *The AllDifferent constraint: A survey*. arXiv:cs/0105015.

use crate::constraint::{Constraint, PropagationResult};
use crate::model::domain::TrailedDomains;
use crate::model::variable::VariableId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};

/// Every `REGIN_INTERVAL`-th [`AllDifferent::propagate`] call on a given instance runs the full
/// Régin matching+SCC pass; the others run only the cheap fixed-value pass
/// ([`prune_fixed_values`]). Benchmarking the full-strength-every-call version (see
/// `plan/00-STATUS.md`, part C) measured a ~10x per-call throughput cost that an index-based
/// rewrite narrowed to ~2.7-4x but didn't close — most calls during search don't actually sit on
/// a Hall set, so paying the O(N*E) matching + O(N+E) SCC/reachability cost on every single call
/// buys little. Throttling to every 4th call trades a bounded amount of missed Hall-set pruning
/// (never *unsound* — see [`AllDifferent::propagate`]'s doc comment) for materially less overhead
/// per search node. Chosen by measurement, not derivation; see
/// `plan/11-search-heuristics-and-global-constraints.md`, section C, for the alternative
/// considered (incremental matching maintenance, not implemented — larger, cross-cutting change).
const REGIN_INTERVAL: u32 = 4;

/// Global constraint enforcing that all variables in its scope take pairwise distinct values.
#[derive(Debug)]
pub struct AllDifferent {
    scope: Vec<VariableId>,
    /// Call counter driving [`REGIN_INTERVAL`]-throttled Régin invocation. Monotonic across the
    /// whole search (not trail/checkpoint-aware) — it's a performance hint, not solver state, so
    /// it doesn't need to roll back on backtrack.
    call_count: AtomicU32,
}

impl Clone for AllDifferent {
    /// Clones the scope; the call counter restarts at 0 (it's a throttling hint, not semantic
    /// state — see `REGIN_INTERVAL`).
    fn clone(&self) -> Self {
        Self {
            scope: self.scope.clone(),
            call_count: AtomicU32::new(0),
        }
    }
}

impl AllDifferent {
    /// Creates a new `AllDifferent` constraint over the given variables.
    ///
    /// # Complexity
    /// Time & Space: O(N) where N is number of variables.
    pub fn new(variables: impl IntoIterator<Item = VariableId>) -> Self {
        Self {
            scope: variables.into_iter().collect(),
            call_count: AtomicU32::new(0),
        }
    }
}

/// Removes already-fixed (singleton-domain) values from every other scope variable, and detects
/// the immediate conflict of two variables both fixed to the same value — the pairwise filtering
/// `AllDifferent` used before Régin's algorithm was added. Cheap (O(scope.len())) relative to full
/// GAC, and run on every [`AllDifferent::propagate`] call regardless of [`REGIN_INTERVAL`]
/// throttling: it catches the common "two fixed variables collide" conflict without paying for a
/// full matching, and its pruning feeds directly into whichever pass (cheap-only or full Régin)
/// runs next.
///
/// # Complexity
/// Time & Space: O(N) where N = `scope.len()`.
fn prune_fixed_values(
    domains: &mut TrailedDomains,
    scope: &[VariableId],
) -> Result<bool, PropagationResult> {
    let mut changed = false;

    let mut fixed_values = HashSet::new();
    for &var in scope {
        if let Some(domain) = domains.get(&var)
            && domain.len() == 1
            && let Some(val) = domain.min()
            && !fixed_values.insert(val)
        {
            return Err(PropagationResult::Conflict);
        }
    }

    if fixed_values.is_empty() {
        return Ok(false);
    }

    for &var in scope {
        if !domains.get(&var).is_some_and(|d| d.len() > 1) {
            continue;
        }
        let did_change = domains
            .mutate(var, |domain| {
                let mut any = false;
                for &val in &fixed_values {
                    if domain.remove(val) {
                        any = true;
                    }
                }
                any
            })
            .unwrap_or(false);
        if did_change {
            changed = true;
        }
        if domains.get(&var).is_some_and(|d| d.is_empty()) {
            return Err(PropagationResult::Conflict);
        }
    }

    Ok(changed)
}

/// Extends the matching `match_var`/`match_val` by one more variable via an augmenting path
/// (Kuhn's algorithm), starting from `var`.
///
/// Nodes are plain indices rather than a hash-keyed type: variables are `0..var_candidates.len()`
/// (matching [`AllDifferent::scope`]'s order) and values are `0..match_val.len()` (matching the
/// `values` index built by [`AllDifferent::propagate`]). Index-based `Vec` access instead of
/// hashing is a deliberate constant-factor optimization — see the `# Performance` note on
/// `propagate` below.
///
/// Returns `true` if `var` could be matched to some value in `var_candidates[var]` — either
/// directly to a free value, or by displacing another variable onto one of its other candidate
/// values.
///
/// # Complexity
/// Time: O(E) per call (E = total candidate edges), O(V * E) over the full matching
/// ([`AllDifferent::propagate`] calls this once per scope variable).
fn try_augment(
    var: usize,
    var_candidates: &[Vec<usize>],
    match_var: &mut [Option<usize>],
    match_val: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for &val in &var_candidates[var] {
        if visited[val] {
            continue;
        }
        visited[val] = true;
        match match_val[val] {
            None => {
                match_val[val] = Some(var);
                match_var[var] = Some(val);
                return true;
            }
            Some(other_var) => {
                if try_augment(other_var, var_candidates, match_var, match_val, visited) {
                    match_val[val] = Some(var);
                    match_var[var] = Some(val);
                    return true;
                }
            }
        }
    }
    false
}

/// Computes strongly connected components (Tarjan's algorithm) of the directed graph `adj` (node
/// `i`'s successors are `adj[i]`), returning each node's component id. Two nodes have the same id
/// iff they're mutually reachable.
///
/// # Complexity
/// Time & Space: O(V + E) where V/E are the node/edge counts of `adj`.
///
/// # Reference
/// Tarjan, R. E. (1972). *Depth-first search and linear graph algorithms*. SIAM Journal on
/// Computing, 1(2), 146-160.
fn compute_scc(adj: &[Vec<usize>]) -> Vec<usize> {
    const UNVISITED: usize = usize::MAX;

    struct State {
        counter: usize,
        indices: Vec<usize>,
        low_links: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        scc_id: Vec<usize>,
        next_scc_id: usize,
    }

    fn strongconnect(node: usize, adj: &[Vec<usize>], state: &mut State) {
        state.indices[node] = state.counter;
        state.low_links[node] = state.counter;
        state.counter += 1;
        state.stack.push(node);
        state.on_stack[node] = true;

        for &next in &adj[node] {
            if state.indices[next] == UNVISITED {
                strongconnect(next, adj, state);
                state.low_links[node] = state.low_links[node].min(state.low_links[next]);
            } else if state.on_stack[next] {
                state.low_links[node] = state.low_links[node].min(state.indices[next]);
            }
        }

        if state.low_links[node] == state.indices[node] {
            let scc_id = state.next_scc_id;
            state.next_scc_id += 1;
            loop {
                let w = state.stack.pop().expect("node's own SCC root is on stack");
                state.on_stack[w] = false;
                state.scc_id[w] = scc_id;
                if w == node {
                    break;
                }
            }
        }
    }

    let total = adj.len();
    let mut state = State {
        counter: 0,
        indices: vec![UNVISITED; total],
        low_links: vec![0; total],
        on_stack: vec![false; total],
        stack: Vec::new(),
        scc_id: vec![UNVISITED; total],
        next_scc_id: 0,
    };
    for node in 0..total {
        if state.indices[node] == UNVISITED {
            strongconnect(node, adj, &mut state);
        }
    }
    state.scc_id
}

/// Computes, for every node, whether it can reach some node in `sources` via a directed path in
/// the graph `adj` (`sources` themselves count, via the zero-length path).
///
/// Implemented as a multi-source BFS on the *reverse* graph, seeded from `sources`.
///
/// # Complexity
/// Time & Space: O(V + E).
fn reaches_any(sources: impl IntoIterator<Item = usize>, adj: &[Vec<usize>]) -> Vec<bool> {
    let total = adj.len();
    let mut rev_adj: Vec<Vec<usize>> = vec![Vec::new(); total];
    for (from, tos) in adj.iter().enumerate() {
        for &to in tos {
            rev_adj[to].push(from);
        }
    }

    let mut reached = vec![false; total];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for source in sources {
        if !reached[source] {
            reached[source] = true;
            queue.push_back(source);
        }
    }
    while let Some(node) = queue.pop_front() {
        for &pred in &rev_adj[node] {
            if !reached[pred] {
                reached[pred] = true;
                queue.push_back(pred);
            }
        }
    }
    reached
}

impl Constraint for AllDifferent {
    fn name(&self) -> &str {
        "AllDifferent"
    }

    fn scope(&self) -> &[VariableId] {
        &self.scope
    }

    fn is_satisfied(&self, assignment: &HashMap<VariableId, i64>) -> bool {
        let mut seen = HashSet::new();
        for var in &self.scope {
            if let Some(&val) = assignment.get(var)
                && !seen.insert(val)
            {
                return false; // Duplicate value found
            }
        }
        true
    }

    /// Enforces generalized arc consistency (GAC) via Régin's algorithm: a bipartite maximum
    /// matching between scope variables and their candidate values, followed by a directed-graph
    /// analysis of that matching to identify exactly the (variable, value) edges that cannot
    /// participate in *any* maximum matching — those are pruned.
    ///
    /// This is strictly stronger than pairwise/fixed-value filtering: it also removes values
    /// that remain individually possible for a variable but can never coexist with a complete
    /// distinct assignment of the rest of the scope (a "Hall set" — e.g. two variables whose
    /// combined domain is exactly two values forces every *other* variable off both of them,
    /// even though neither of those two variables is individually fixed).
    ///
    /// By Berge's alternating-path theorem, a non-matching edge `(x, v)` belongs to some maximum
    /// matching iff `x` and `v` lie on a common cycle in the matching's directed graph (same
    /// SCC), or `v` lies on an alternating path to a free/unmatched value. Both are computed
    /// below (`compute_scc`, `reaches_any`); an edge failing both is pruned.
    ///
    /// The full matching+SCC pass only runs every `REGIN_INTERVAL`-th call on this instance;
    /// `prune_fixed_values` (cheap fixed-value filtering) always runs. This is a deliberate,
    /// sound weakening — a skipped call still performs valid (if not maximally strong)
    /// propagation, it just doesn't always catch every Hall-set-only inconsistency the moment it
    /// appears. See `REGIN_INTERVAL`'s doc comment for why.
    ///
    /// # Performance
    /// Variables and candidate values are mapped to plain `0..n`/`0..m` indices once up front
    /// (`var_candidates`, `values`) so the matching, SCC, and reachability passes operate on
    /// `Vec`-indexed arrays instead of hashing a `VariableId`/value-tagged node type on every
    /// access. This was added after benchmarking showed the straightforward hash-map-keyed
    /// version costing roughly 10x the per-node throughput of the pairwise filtering it replaced
    /// (see `plan/00-STATUS.md` and `plan/11-search-heuristics-and-global-constraints.md`,
    /// section C) — the asymptotic complexity is unchanged, only the constant factor.
    ///
    /// # Complexity
    /// Time: O(N * E) for the matching (Kuhn's augmenting-path algorithm; Hopcroft-Karp would
    /// give O(sqrt(N) * E) but isn't implemented here — see `plan/11-search-heuristics-and-global-constraints.md`),
    /// plus O(N + E) each for the SCC and reachability passes, where N = `scope.len()` and E =
    /// sum of candidate-value counts over the scope. Enumerates every candidate value per
    /// variable ([`crate::model::domain::Domain::values`]), so a scope with very large
    /// integer-range domains is proportionally expensive.
    /// Space: O(N + E) for the matching and directed graph.
    ///
    /// # Reference
    /// Régin, J. C. (1994). *A filtering algorithm for constraints of difference in CSPs*.
    /// AAAI-94, 362-367 (matching via Kuhn, H. W. (1955), *The Hungarian method for the
    /// assignment problem*; SCC decomposition via Tarjan, R. E. (1972)).
    fn propagate(&self, domains: &mut TrailedDomains) -> PropagationResult {
        let n = self.scope.len();
        if n == 0 {
            return PropagationResult::Success { changed: false };
        }

        let mut changed = match prune_fixed_values(domains, &self.scope) {
            Ok(changed) => changed,
            Err(conflict) => return conflict,
        };

        // Throttle the expensive full pass (see `REGIN_INTERVAL`'s doc comment); the calls in
        // between rely on the cheap fixed-value pass above alone.
        let call_index = self.call_count.fetch_add(1, Ordering::Relaxed);
        if !call_index.is_multiple_of(REGIN_INTERVAL) {
            return PropagationResult::Success { changed };
        }

        // Assign each distinct candidate value a dense index 0..m, shared across variables so
        // matching/SCC/reachability can address it as a plain `Vec` slot.
        let mut val_index: HashMap<i64, usize> = HashMap::new();
        let mut values: Vec<i64> = Vec::new();
        let mut var_candidates: Vec<Vec<usize>> = Vec::with_capacity(n);
        for &var in &self.scope {
            let mut candidates = Vec::new();
            if let Some(domain) = domains.get(&var) {
                for val in domain.values() {
                    let idx = *val_index.entry(val).or_insert_with(|| {
                        values.push(val);
                        values.len() - 1
                    });
                    candidates.push(idx);
                }
            }
            var_candidates.push(candidates);
        }
        let m = values.len();

        // Bipartite maximum matching (Kuhn's augmenting-path algorithm): match each scope
        // variable to a distinct candidate value. If some variable cannot be matched, no
        // all-different assignment exists for the current domains.
        let mut match_var: Vec<Option<usize>> = vec![None; n];
        let mut match_val: Vec<Option<usize>> = vec![None; m];
        let mut visited = vec![false; m];
        for var in 0..n {
            visited.iter_mut().for_each(|v| *v = false);
            if !try_augment(
                var,
                &var_candidates,
                &mut match_var,
                &mut match_val,
                &mut visited,
            ) {
                return PropagationResult::Conflict;
            }
        }

        // Directed graph of the matching: variables are nodes `0..n`, values are nodes
        // `n..n+m`. Matched edges point value -> variable, unmatched edges point
        // variable -> value.
        let total = n + m;
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); total];
        for var in 0..n {
            let matched_val = match_var[var].expect("every variable was matched above");
            for &val in &var_candidates[var] {
                let val_node = n + val;
                if val == matched_val {
                    adj[val_node].push(var);
                } else {
                    adj[var].push(val_node);
                }
            }
        }

        let scc_id = compute_scc(&adj);
        let free_value_nodes = (0..m)
            .filter(|&val| match_val[val].is_none())
            .map(|val| n + val);
        let reaches_free = reaches_any(free_value_nodes, &adj);

        // Prune (var, val) when neither sufficient condition for "usable in some maximum
        // matching" holds. The matched value is never a pruning candidate, so `mutate` below
        // always leaves at least one value behind — the domain can't become empty from this
        // constraint's own pruning. `changed` already reflects `prune_fixed_values` above; this
        // loop only ever adds to it.
        for var in 0..n {
            let matched_val = match_var[var].expect("every variable was matched above");
            let var_scc = scc_id[var];
            let to_remove: Vec<i64> = var_candidates[var]
                .iter()
                .copied()
                .filter(|&val| {
                    if val == matched_val {
                        return false;
                    }
                    let val_node = n + val;
                    scc_id[val_node] != var_scc && !reaches_free[val_node]
                })
                .map(|val| values[val])
                .collect();
            if to_remove.is_empty() {
                continue;
            }
            let did_change = domains
                .mutate(self.scope[var], |domain| {
                    let mut any = false;
                    for &val in &to_remove {
                        if domain.remove(val) {
                            any = true;
                        }
                    }
                    any
                })
                .unwrap_or(false);
            if did_change {
                changed = true;
            }
        }

        PropagationResult::Success { changed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::Domain;
    use crate::model::variable::Variable;
    use crate::propagation::graph::ConstraintGraph;
    use crate::solver::{BacktrackingSolver, SolverOptions};
    use std::sync::Arc;

    /// Régin's algorithm can prune a value that's individually still possible for a variable but
    /// can never coexist with a distinct assignment of the rest of the scope — the classic Hall
    /// set case that pairwise/fixed-value filtering alone cannot detect.
    ///
    /// a,b in {1,2} (neither fixed — a Hall set of size 2 over {1,2}), c in {1,2,3}. c must lose
    /// both 1 and 2 (only a and b can ever hold them between the two of them), leaving c = {3}.
    /// The pre-Régin implementation left all three domains untouched here, since none is a
    /// singleton.
    #[test]
    fn test_propagate_prunes_hall_set_beyond_pairwise_filtering() {
        let mut domains = HashMap::new();
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        domains.insert(a, Domain::range(1, 2));
        domains.insert(b, Domain::range(1, 2));
        domains.insert(c, Domain::range(1, 3));
        let mut trailed = TrailedDomains::new(domains);

        let constraint = AllDifferent::new([a, b, c]);
        let result = constraint.propagate(&mut trailed);

        assert_eq!(result, PropagationResult::Success { changed: true });
        assert_eq!(trailed.get(&a).unwrap().values(), vec![1, 2]);
        assert_eq!(trailed.get(&b).unwrap().values(), vec![1, 2]);
        assert_eq!(
            trailed.get(&c).unwrap().values(),
            vec![3],
            "c can never take 1 or 2: a,b (a Hall set) exhaust both between them"
        );
    }

    /// The full Régin pass only runs every `REGIN_INTERVAL`-th call on a given instance (see its
    /// doc comment); the calls in between rely on `prune_fixed_values` alone, which cannot see a
    /// Hall set that isn't also a fixed-value collision. Re-running the exact Hall-set scenario
    /// from `test_propagate_prunes_hall_set_beyond_pairwise_filtering` against fresh domains each
    /// time, but the *same* `AllDifferent` instance (so its call counter keeps advancing), the
    /// Hall-set pruning must appear on call 1 and call 5 (indices 0 and 4), and be absent on
    /// calls 2-4 (indices 1-3).
    #[test]
    fn test_propagate_throttles_full_regin_pass_to_every_interval_th_call() {
        let a = VariableId(0);
        let b = VariableId(1);
        let c = VariableId(2);
        let constraint = AllDifferent::new([a, b, c]);

        let fresh_domains = || {
            let mut domains = HashMap::new();
            domains.insert(a, Domain::range(1, 2));
            domains.insert(b, Domain::range(1, 2));
            domains.insert(c, Domain::range(1, 3));
            TrailedDomains::new(domains)
        };

        let mut pruned_c_on_call = Vec::new();
        for call in 1..=(REGIN_INTERVAL as usize + 1) {
            let mut trailed = fresh_domains();
            constraint.propagate(&mut trailed);
            if trailed.get(&c).unwrap().values() == vec![3] {
                pruned_c_on_call.push(call);
            }
        }

        assert_eq!(
            pruned_c_on_call,
            vec![1, REGIN_INTERVAL as usize + 1],
            "full Régin pass (and thus the Hall-set pruning) should fire on call 1 and call \
             REGIN_INTERVAL+1, not the calls in between"
        );
    }

    /// A lone variable (or any variable whose domain isn't part of a Hall set) must keep its full
    /// domain: any single candidate value trivially extends to a maximum matching, regardless of
    /// which one the internal matching step happens to pick first.
    #[test]
    fn test_propagate_keeps_consistent_values() {
        let mut domains = HashMap::new();
        let x = VariableId(0);
        let y = VariableId(1);
        let z = VariableId(2);
        domains.insert(x, Domain::range(1, 3));
        domains.insert(y, Domain::range(1, 3));
        domains.insert(z, Domain::range(1, 3));
        let mut trailed = TrailedDomains::new(domains);

        let constraint = AllDifferent::new([x, y, z]);
        let result = constraint.propagate(&mut trailed);

        assert_eq!(result, PropagationResult::Success { changed: false });
        for &var in &[x, y, z] {
            assert_eq!(trailed.get(&var).unwrap().values(), vec![1, 2, 3]);
        }
    }

    /// Pigeonhole: 4 variables, only 3 possible values between them -> no matching of size 4
    /// exists, so propagation must report `Conflict` (not merely fail to prune).
    #[test]
    fn test_propagate_detects_pigeonhole_conflict() {
        let mut domains = HashMap::new();
        let vars: Vec<VariableId> = (0..4).map(VariableId).collect();
        for &v in &vars {
            domains.insert(v, Domain::range(1, 3));
        }
        let mut trailed = TrailedDomains::new(domains);

        let constraint = AllDifferent::new(vars);
        let result = constraint.propagate(&mut trailed);

        assert_eq!(result, PropagationResult::Conflict);
    }

    #[test]
    fn test_solve_simple_all_different() {
        let mut graph = ConstraintGraph::new();
        let v1 = VariableId(1);
        let v2 = VariableId(2);
        let v3 = VariableId(3);

        graph.add_variable(Variable::new(v1, "x"), Domain::range(1, 3));
        graph.add_variable(Variable::new(v2, "y"), Domain::range(1, 3));
        graph.add_variable(Variable::new(v3, "z"), Domain::range(1, 3));
        graph.add_constraint(Arc::new(AllDifferent::new([v1, v2, v3])));
        let graph = graph.finalize().unwrap();

        let solver = BacktrackingSolver::new();
        let outcome = solver.solve(&graph, &SolverOptions::default());

        let solution = outcome.solution.expect("should find a feasible solution");
        let mut values: Vec<i64> = solution.assignment.values().copied().collect();
        values.sort_unstable();
        assert_eq!(values, vec![1, 2, 3]);
    }
}
