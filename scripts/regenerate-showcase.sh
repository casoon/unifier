#!/usr/bin/env bash
# Captures the output of the example programs in examples/ into examples/output/, which the
# website showcase (site/) renders. Run after changing an example or the solver, then commit
# the updated files. Needs the pathwise checkout the Cargo.toml path dependency points to.
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p examples/output
for example in nqueens map_coloring job_shop scheduling_demo; do
  cargo run --quiet --release --example "$example" > "examples/output/$example.txt"
  echo "examples/output/$example.txt"
done
