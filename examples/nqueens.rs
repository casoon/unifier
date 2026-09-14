//! Eight queens as a pure constraint satisfaction problem (CSP), solved with
//! `BacktrackingSolver`.
//!
//! `queens[row]` holds the column of the queen in that row, so rows differ by construction.
//! Columns must differ (`AllDifferent`), and so must both diagonals (`NotEqual` with the row
//! distance as offset, in both directions).
//!
//! Run with `cargo run --example nqueens`.

use std::sync::Arc;
use unifier::VariableId;
use unifier::constraint::NotEqual;
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, SolverOptions};

const N: i64 = 8;

fn main() {
    let mut builder = ModelBuilder::new();
    let queens: Vec<VariableId> = (0..N)
        .map(|row| builder.new_var(format!("q{row}"), 1..=N))
        .collect();

    builder.add_all_different(queens.clone());
    for i in 0..queens.len() {
        for j in (i + 1)..queens.len() {
            let distance = (j - i) as i64;
            builder.add_constraint(Arc::new(NotEqual::with_offset(
                queens[i], queens[j], distance,
            )));
            builder.add_constraint(Arc::new(NotEqual::with_offset(
                queens[i], queens[j], -distance,
            )));
        }
    }

    let graph = builder.build().expect("N-Queens model validates");
    println!(
        "{N}-Queens: {} variables, {} constraints",
        graph.variables().len(),
        graph.constraints().len()
    );

    let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());
    println!("Status: {:?}\n", outcome.status);

    let Some(solution) = outcome.solution else {
        println!("No placement found.");
        return;
    };
    for &queen in &queens {
        let column = solution.assignment[&queen];
        let row: String = (1..=N)
            .map(|c| if c == column { " Q" } else { " ." })
            .collect();
        println!("{row}");
    }
}
