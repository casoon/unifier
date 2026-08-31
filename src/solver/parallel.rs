//! Parallel Multi-Threaded Portfolio Solver.
//!
//! Spawns concurrent solver strategies (Backtracking, Local Search, LNS) in parallel threads,
//! returning the first or best feasible solution and cancelling remaining threads upon completion.
//!
//! References:
//! - Gomes, C. P., & Selman, B. (2001). *Algorithm portfolios*. Artificial Intelligence, 126(1-2), 43-62.
//! - Hamadi, Y., & Sais, L. (2009). *ManySAT: a parallel SAT solver*. JSAT, 6(4), 245-262.

use crate::propagation::graph::ConstraintGraph;
use crate::solver::backtracking::BacktrackingSolver;
use crate::solver::cancellation::CancellationToken;
use crate::solver::lns::LnsSolver;
use crate::solver::local_search::LocalSearchSolver;
use crate::solver::{AbortReason, SolveResult, SolverOptions};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Parallel portfolio search manager.
#[derive(Debug, Default)]
pub struct ParallelSolver;

impl ParallelSolver {
    /// Creates a new parallel solver manager instance.
    pub fn new() -> Self {
        Self
    }

    /// Runs parallel portfolio search over `graph` using concurrent solver threads.
    ///
    /// Reports `Infeasible` only if a worker actually proved it; if every worker merely ran out
    /// of time, budget, or was cancelled without finding a feasible assignment, this returns
    /// `Aborted` instead. Workers are coordinated via a solver-owned cancellation token, so a run
    /// never mutates a token the caller supplied via `options.cancellation_token` — that token is
    /// only observed, never cancelled by this solver.
    ///
    /// # Complexity
    /// Time: Min time across all parallel search strategies.
    /// Space: O(P * N * D) where P is thread count.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        let (tx, rx) = mpsc::channel();

        // Coordinates worker shutdown internally; the caller's own token (if any) is observed
        // below but never mutated, so it stays reusable for the caller's other operations.
        let internal_token = CancellationToken::new();

        let mut thread_options = options.clone();
        thread_options.cancellation_token = Some(internal_token.clone());

        // Worker 1: Backtracking solver
        let graph1 = graph.clone();
        let opts1 = thread_options.clone();
        let tx1 = tx.clone();
        thread::spawn(move || {
            let solver = BacktrackingSolver::new();
            let res = solver.solve(&graph1, &opts1);
            // The receiver may already be dropped if `solve` returned after another worker's
            // result was accepted first; a send failure here is expected, not an error.
            let _ = tx1.send(res);
        });

        // Worker 2: Local Search solver
        let graph2 = graph.clone();
        let opts2 = thread_options.clone();
        let tx2 = tx.clone();
        thread::spawn(move || {
            let solver = LocalSearchSolver::default();
            let res = solver.solve(&graph2, &opts2);
            // See worker 1: an already-dropped receiver is an expected outcome, not an error.
            let _ = tx2.send(res);
        });

        // Worker 3: LNS solver
        let graph3 = graph.clone();
        let opts3 = thread_options.clone();
        let tx3 = tx;
        thread::spawn(move || {
            let solver = LnsSolver::default();
            let res = solver.solve(&graph3, &opts3);
            // See worker 1: an already-dropped receiver is an expected outcome, not an error.
            let _ = tx3.send(res);
        });

        // Wait for the first feasible result, or until every worker has reported.
        let mut responses_count = 0;
        let mut proven_infeasible = false;

        while responses_count < 3 {
            if let Ok(result) = rx.recv_timeout(Duration::from_millis(50)) {
                responses_count += 1;
                match result {
                    SolveResult::Feasible { .. } => {
                        internal_token.cancel();
                        return result;
                    }
                    SolveResult::Infeasible => proven_infeasible = true,
                    SolveResult::Aborted { .. } => {}
                }
            } else if options
                .cancellation_token
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                break;
            }
        }

        internal_token.cancel();

        if proven_infeasible {
            // At least one complete solver (Backtracking, or LNS/Local Search's own immediate
            // empty-domain check) exhaustively proved unsatisfiability.
            SolveResult::Infeasible
        } else if options
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            SolveResult::Aborted {
                reason: AbortReason::Cancelled,
            }
        } else {
            SolveResult::Aborted {
                reason: AbortReason::Timeout,
            }
        }
    }
}
