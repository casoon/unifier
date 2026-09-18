//! Parallel Multi-Threaded Portfolio Solver.
//!
//! Spawns concurrent solver strategies (Backtracking, Local Search, LNS, Branch & Bound) in
//! parallel threads, coordinated through a [`SharedIncumbent`] (see
//! `plan/13-anytime-portfolio.md`): every worker contributes improving solutions to it, and
//! Branch & Bound additionally bounds its own search against whatever the others have found.
//! The final result is read from the shared incumbent — the best solution found by *any* worker
//! — rather than whichever worker happened to report first.
//!
//! References:
//! - Gomes, C. P., & Selman, B. (2001). *Algorithm portfolios*. Artificial Intelligence, 126(1-2), 43-62.
//! - Hamadi, Y., & Sais, L. (2009). *ManySAT: a parallel SAT solver*. JSAT, 6(4), 245-262.

use crate::propagation::graph::ValidatedGraph;
use crate::solver::backtracking::BacktrackingSolver;
use crate::solver::branch_and_bound::BranchAndBoundSolver;
use crate::solver::cancellation::CancellationToken;
use crate::solver::lns::LnsSolver;
use crate::solver::local_search::LocalSearchSolver;
use crate::solver::shared_incumbent::SharedIncumbent;
use crate::solver::{
    AbortReason, SearchStatistics, Solution, SolveOutcome, SolveStatus, SolverOptions,
};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Parallel portfolio search manager.
#[derive(Debug, Default)]
pub struct ParallelSolver;

impl ParallelSolver {
    /// Creates a new parallel solver manager instance.
    pub fn new() -> Self {
        Self
    }

    /// Runs parallel portfolio search over `graph` using concurrent solver threads
    /// (Backtracking, Local Search, LNS, Branch & Bound), coordinated through a
    /// [`SharedIncumbent`] shared across all four (see the module doc comment).
    ///
    /// The returned solution is the best one found by *any* worker, not necessarily the one that
    /// reported first. Status is [`SolveStatus::Optimal`] only if Branch & Bound — the only
    /// worker able to prove it — actually did; [`SolveStatus::Infeasible`] only if a worker
    /// actually proved it (if every worker merely ran out of time/budget/was cancelled without
    /// finding anything, this returns `Aborted` instead — see
    /// `plan/08-project-evaluation.md`, finding #3). Workers are coordinated via a solver-owned
    /// cancellation token, so a run never mutates a token the caller supplied via
    /// `options.cancellation_token` — that token is only observed, never cancelled by this
    /// solver. Every spawned worker thread is joined before this method returns — none are left
    /// running in the background.
    ///
    /// # Complexity
    /// Time: bounded by the slowest of the four workers to actually finish (not just report),
    /// since all are joined before returning.
    /// Space: O(P * N * D) where P is thread count.
    pub fn solve(&self, graph: &ValidatedGraph, options: &SolverOptions) -> SolveOutcome {
        let start_time = Instant::now();
        let (tx, rx) = mpsc::channel();

        // Coordinates worker shutdown internally; the caller's own token (if any) is observed
        // below but never mutated, so it stays reusable for the caller's other operations.
        let internal_token = CancellationToken::new();
        // A caller who already holds a solution passes it in through `options.shared_incumbent`,
        // and the portfolio then starts bounded by it instead of from nothing — the same head
        // start the workers give each other, only from outside. Improvements flow back into that
        // handle, so the caller sees them too.
        let shared_incumbent = options
            .shared_incumbent
            .clone()
            .unwrap_or_else(SharedIncumbent::new);

        let mut thread_options = options.clone();
        thread_options.cancellation_token = Some(internal_token.clone());
        thread_options.shared_incumbent = Some(shared_incumbent.clone());

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(4);

        // Worker 1: Backtracking solver (CSP-focused: dom/wdeg variable ordering).
        {
            let graph1 = graph.clone();
            let opts1 = thread_options.clone();
            let tx1 = tx.clone();
            handles.push(thread::spawn(move || {
                let res = BacktrackingSolver::new().solve(&graph1, &opts1);
                // The receiver may already be dropped if `solve` returned after another worker's
                // result was accepted first; a send failure here is expected, not an error.
                let _ = tx1.send(res);
            }));
        }

        // Worker 2: Local Search solver.
        {
            let graph2 = graph.clone();
            let opts2 = thread_options.clone();
            let tx2 = tx.clone();
            handles.push(thread::spawn(move || {
                let res = LocalSearchSolver::default().solve(&graph2, &opts2);
                let _ = tx2.send(res);
            }));
        }

        // Worker 3: LNS solver.
        {
            let graph3 = graph.clone();
            let opts3 = thread_options.clone();
            let tx3 = tx.clone();
            handles.push(thread::spawn(move || {
                let res = LnsSolver::default().solve(&graph3, &opts3);
                let _ = tx3.send(res);
            }));
        }

        // Worker 4: Branch & Bound solver -- the only one able to *prove* optimality, and the
        // one that benefits most from a head start: seeded by whichever of the other three
        // finds a decent solution first (see `SharedIncumbent`), it can bound its search against
        // that instead of starting from nothing.
        {
            let graph4 = graph.clone();
            let opts4 = thread_options.clone();
            let tx4 = tx;
            handles.push(thread::spawn(move || {
                let res = BranchAndBoundSolver::new().solve(&graph4, &opts4);
                let _ = tx4.send(res);
            }));
        }

        // Wait for a worker to prove optimality, or until every worker has reported (whichever
        // first) -- this only decides *when* to stop polling, not which solution wins (that's
        // `shared_incumbent`, read below).
        let mut responses_count = 0;
        let mut proven_infeasible = false;
        let mut proven_optimal = false;
        let mut nodes_expanded = 0u64;

        while responses_count < handles.len() {
            if let Ok(outcome) = rx.recv_timeout(Duration::from_millis(50)) {
                responses_count += 1;
                nodes_expanded = nodes_expanded.saturating_add(outcome.statistics.nodes_expanded);
                match outcome.status {
                    SolveStatus::Optimal => {
                        proven_optimal = true;
                        break;
                    }
                    SolveStatus::Infeasible => proven_infeasible = true,
                    SolveStatus::Feasible | SolveStatus::Aborted(_) => {}
                }
            } else if options
                .cancellation_token
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                break;
            }
        }

        // Signal every worker to stop, then join all of them -- unconditionally, on every exit
        // path (early-optimal, all-reported, or caller-cancelled). No worker thread is left
        // running in the background after this call returns.
        internal_token.cancel();
        for handle in handles {
            let _ = handle.join();
        }

        // Every worker has now fully finished (and therefore already attempted its `send`);
        // drain any results not yet read above so the final statistics/status reflect all four,
        // not just however many were read before the polling loop stopped.
        while let Ok(outcome) = rx.try_recv() {
            nodes_expanded = nodes_expanded.saturating_add(outcome.statistics.nodes_expanded);
            match outcome.status {
                SolveStatus::Optimal => proven_optimal = true,
                SolveStatus::Infeasible => proven_infeasible = true,
                SolveStatus::Feasible | SolveStatus::Aborted(_) => {}
            }
        }

        let statistics = SearchStatistics {
            nodes_expanded,
            elapsed: start_time.elapsed(),
        };

        // An infeasible assignment is not a solution: it must never leave here as `Feasible`,
        // however a worker came to offer it.
        match shared_incumbent
            .best()
            .filter(|(_, score)| score.is_feasible())
        {
            Some((assignment, score)) => {
                let solution = Solution { assignment, score };
                if proven_optimal {
                    SolveOutcome::optimal(solution, statistics, Some(score))
                } else {
                    SolveOutcome::feasible(solution, statistics, None)
                }
            }
            None if proven_infeasible => {
                // At least one complete solver (Backtracking, or LNS/Local Search's own
                // immediate empty-domain check) exhaustively proved unsatisfiability.
                SolveOutcome::infeasible(statistics)
            }
            None if options
                .cancellation_token
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled) =>
            {
                SolveOutcome::aborted(AbortReason::Cancelled, statistics)
            }
            None => SolveOutcome::aborted(AbortReason::Timeout, statistics),
        }
    }
}
