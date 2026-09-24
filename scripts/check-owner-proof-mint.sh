#!/usr/bin/env bash
# Owner-proof constructor placement (#263/#208). The lexical guard reserves
# every ::mint reference for the exact channel-adapter allowlist, including
# aliases and function pointers. It is not Rust name resolution or macro
# expansion. Tests use the distinct #[cfg(test)] test_new constructor.
set -euo pipefail

cd "$(dirname "$0")/.."
node scripts/check-store-boundaries.mjs owner-proofs crates/openspine-kernel/src
echo "check-owner-proof-mint: source-level mint references are adapter-confined."
