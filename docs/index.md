---
title: Overview
description: What unifier does, where it stops, and how this documentation is organised.
order: 0
---

unifier is a Rust crate for constraint satisfaction problems (CSP) and constraint optimization
problems (COP): integer variables with domains, constraints between them and, for optimization,
weighted objectives. Typical instances are scheduling, timetabling and resource allocation.

These problems are generally NP-hard, so there is no single best algorithm. unifier ships several
interchangeable solver strategies and is designed as an anytime solver: find a valid solution
fast, improve it, and stop whenever a time limit, node budget or cancellation says so.

## What it covers

- A constraint graph of variables, domains, constraints and objectives, validated before any
  solver sees it.
- Sixteen built-in constraints in 0.5.1, including the global constraints `AllDifferent`,
  `NoOverlap` and `Cumulative` with dedicated propagation.
- Hard/soft scoring: hard constraints decide feasibility, weighted soft terms on three
  lexicographic levels rank feasible solutions.
- Five solvers: Backtracking, Branch & Bound, Local Search, Large Neighbourhood Search and a
  parallel portfolio.
- Scheduling primitives (`Interval`, `Activity`, `Resource`, `Group`) that compile into
  constraints.
- An incremental feasibility check that evaluates only the constraints touched by a change.

## Where it stops

The crate is early (`0.5.x`). Not covered yet: general unsat cores, `serde`-based model or
solution serialization, a command-line interface, and independent verification against
production-scale scheduling scenarios. Evaluate it accordingly before relying on it for
production planning.

## How the docs are organised

- **Getting started**: add the crate and solve a first model.
- **Guides**: [modelling](guides/modelling/), [solvers](guides/solvers/) and how unifier
  [relates to pathwise](guides/pathwise/).
- **Reference**: an overview of the public modules. Item-level documentation lives on
  [docs.rs](https://docs.rs/unifier/0.5.1/unifier/).
