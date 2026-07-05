#!/usr/bin/env bash
set -euo pipefail

# Build and run the benchmark harness
cargo run --release --bin openspine -- --benchmark
