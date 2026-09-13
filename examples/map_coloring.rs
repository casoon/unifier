//! Map colouring of the Australian states and territories, the classic CSP from Russell &
//! Norvig, *Artificial Intelligence: A Modern Approach*, chapter 6.
//!
//! Every region gets a colour; neighbouring regions must differ (`NotEqual`). With three colours
//! the solver finds a colouring. With two it proves that none exists: Western Australia,
//! Northern Territory and South Australia all border each other.
//!
//! Run with `cargo run --example map_coloring`.

use unifier::VariableId;
use unifier::dsl::ModelBuilder;
use unifier::solver::{BacktrackingSolver, SolveStatus, SolverOptions};

const REGIONS: [&str; 7] = ["WA", "NT", "SA", "Q", "NSW", "V", "T"];
const BORDERS: [(&str, &str); 9] = [
    ("WA", "NT"),
    ("WA", "SA"),
    ("NT", "SA"),
    ("NT", "Q"),
    ("SA", "Q"),
    ("SA", "NSW"),
    ("SA", "V"),
    ("Q", "NSW"),
    ("NSW", "V"),
];
const COLOURS: [&str; 3] = ["red", "green", "blue"];

fn solve(colours: usize) {
    let mut builder = ModelBuilder::new();
    let vars: Vec<VariableId> = REGIONS
        .iter()
        .map(|region| builder.new_var(*region, 0..=(colours as i64 - 1)))
        .collect();
    let var = |name: &str| {
        vars[REGIONS
            .iter()
            .position(|r| *r == name)
            .expect("known region")]
    };
    for (a, b) in BORDERS {
        builder.add_not_equal(var(a), var(b));
    }

    let graph = builder.build().expect("map colouring model validates");
    let outcome = BacktrackingSolver::new().solve(&graph, &SolverOptions::default());

    println!("{colours} colours: {:?}", outcome.status);
    match (outcome.status, outcome.solution) {
        (_, Some(solution)) => {
            for (region, &v) in REGIONS.iter().zip(&vars) {
                println!(
                    "  {region:<4} {}",
                    COLOURS[solution.assignment[&v] as usize]
                );
            }
        }
        (SolveStatus::Infeasible, None) => {
            println!("  No colouring exists: WA, NT and SA border each other.");
        }
        (status, None) => println!("  No result: {status:?}"),
    }
}

fn main() {
    println!(
        "Map of Australia: {} regions, {} borders\n",
        REGIONS.len(),
        BORDERS.len()
    );
    solve(3);
    println!();
    solve(2);
}
