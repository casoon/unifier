//! `unifier` — constraint satisfaction / optimization (CSP/COP) modeling
//! and solver framework for Rust, built on [`pathwise`](https://github.com/casoon/pathwise).
//!
//! Concept and scope: see `README.md`. Implementation plan: `plan/`
//! (untracked, local only).
//!
//! Scaffold only — no logic yet. Module layout follows the layered
//! architecture from `plan/01-concept.md`:
//!
//! ```text
//! model        — variable, domain, interval, activity, resource
//! constraint   — equal, not_equal, all_different, no_overlap, cumulative
//! propagation  — constraint graph, propagators
//! score        — hard/soft priority scoring, incremental updates
//! solver       — backtracking, branch_and_bound, local_search, lns
//! dsl          — problem-building surface API
//! ```

// TODO(model): Variable<T>, Domain (BitSet/Range/SparseSet), Interval
// (start/duration/end), Resource (capacity), Activity (interval +
// resource demands), Group/CompositeActivity (shared interval).
// mod model;

// TODO(constraint): Equal, NotEqual, LessThan, AllDifferent, NoOverlap,
// Cumulative, Precedence, ExactlyOne, AtMost, AtLeast, AllowedValues,
// ForbiddenValues. Global constraints — see plan/01-concept.md for MVP
// subset.
// mod constraint;

// TODO(propagation): constraint graph (hypergraph: Variable —
// Constraint — Variable), propagators reusing `pathwise`'s
// arc-consistency primitives where applicable.
// mod propagation;

// TODO(score): hard/soft priority levels, incremental scoring (only
// recompute constraints touched by a changed variable, not the whole
// model).
// mod score;

// TODO(solver): backtracking + MRV/fail-first heuristic, branch_and_bound
// (reusing `pathwise::optimization`), local_search, lns (large
// neighborhood search). Anytime: first valid solution fast, then
// iterative improvement, cancellable at any time.
// mod solver;

// TODO(dsl): problem-building surface API (activity()/resource()/
// constraint() builders) — see plan/01-concept.md for the sketch.
// mod dsl;
