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
use crate::solver::local_search::LocalSearchSolver;
use crate::solver::lns::LnsSolver;
use crate::solver::{SolveResult, SolverOptions};
use std::sync::mpsc;
use std::thread;

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
    /// # Complexity
    /// Time: Min time across all parallel search strategies.
    /// Space: O(P * N * D) where P is thread count.
    pub fn solve(&self, graph: &ConstraintGraph, options: &SolverOptions) -> SolveResult {
        let (tx, rx) = mpsc::channel();
        let cancel_token = options
            .cancellation_token
            .clone()
            .unwrap_or_else(CancellationToken::new);

        let mut thread_options = options.clone();
        thread_options.cancellation_token = Some(cancel_token.clone());

        // Worker 1: Backtracking solver
        let graph1 = graph.clone();
        let opts1 = thread_options.clone();
        let tx1 = tx.clone();
        thread::spawn(move || {
            let solver = BacktrackingSolver::new();
            let res = solver.solve(&graph1, &opts1);
            let _ = tx1.send(res);
        });

        // Worker 2: Local Search solver
        let graph2 = graph.clone();
        let opts2 = thread_options.clone();
        let tx2 = tx.clone();
        thread::spawn(move || {
            let solver = LocalSearchSolver::default();
            let res = solver.solve(&graph2, &opts2);
            let _ = tx2.send(res);
        });

        // Worker 3: LNS solver
        let graph3 = graph.clone();
        let opts3 = thread_options.clone();
        let tx3 = tx;
        thread::spawn(move || {
            let solver = LnsSolver::default();
            let res = solver.solve(&graph3, &opts3);
            let _ = tx3.send(res);
        });

        // Wait for first non-timeout result or best solution
        let mut best_result = SolveResult::Infeasible;
        let mut responses_count = 0;

        while responses_count < 3 {
            if let Ok(result) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
                responses_count += 1;
                if let SolveResult::Feasible { .. } = &result {
                    cancel_token.cancel();
                    return result;
                } else if result != SolveResult::Timeout {
                    best_result = result;
                }
            } else if cancel_token.is_cancelled() {
                break;
            }
        }

        cancel_token.cancel();
        best_result
    }
}
