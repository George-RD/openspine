#!/usr/bin/env bash
# Local gate. Mirrors .github/workflows/ci.yml.
#
# Usage:
#   scripts/check.sh              # everything except a specific change's strict validation
#   scripts/check.sh <change-id>  # also strict-validate one in-flight OpenSpec change
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo clippy --workspace --all-targets -- -D warnings =="
cargo clippy --workspace --all-targets -- -D warnings
# The kernel E2E test spawns the REAL openspine-shell binary; cargo test
# builds shell *tests* but not the shell *binary*, so build it here.
echo "== cargo build openspine-shell binary =="
cargo build -p openspine-shell --bin openspine-shell

echo "== cargo test --workspace =="
cargo test --workspace

echo "== scripts/check-file-sizes.sh =="
scripts/check-file-sizes.sh

echo "== scripts/check-claims.sh =="
scripts/check-claims.sh

echo "== scripts/check-omp-ceremony.sh =="
scripts/check-omp-ceremony.sh

if [ "$#" -ge 1 ]; then
  change_id="$1"
  echo "== openspec validate ${change_id} --strict =="
  /Users/george/repos/openspine/node_modules/.bin/openspec validate "$change_id" --strict
else
  echo "== openspec validate --all --strict =="
  /Users/george/repos/openspine/node_modules/.bin/openspec validate --all --strict
fi

echo "All checks passed."
