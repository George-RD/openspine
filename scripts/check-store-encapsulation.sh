#!/usr/bin/env bash
# Store encapsulation gate (spec #208 D-001/D-004, ticket #218 CONTRACT phase).
# The two Ledger invariants must stay INSIDE the Store interface, and the
# privileged kernel-state layer must not depend on a channel adapter:
#
#   1. BEGIN IMMEDIATE / Deferred transactions are opened ONLY by the private
#      combinators in store/mod.rs (`transaction_with_behavior`); no call site
#      may hand-restate the write-serialization discipline.
#   2. No module outside `store/` locks the raw `Store::conn` (it is a private
#      field). Test-only raw access goes through `test_hooks::with_conn_for_test`;
#      production callers use a Store method.
#   3. `store/` never imports an untrusted channel adapter (`crate::telegram::`),
#      which would invert the trust-boundary dependency direction (AGENTS.md).
set -euo pipefail

cd "$(dirname "$0")/.."

src="crates/openspine-kernel/src"
failed=0

# 1. transaction_with_behavior only in the store/mod.rs combinators.
offenders=$(grep -rln "transaction_with_behavior" "$src" --include='*.rs' \
  | grep -v "^$src/store/mod.rs$" || true)
if [ -n "$offenders" ]; then
  echo "FAIL: transaction_with_behavior outside the store/mod.rs combinators:" >&2
  echo "$offenders" >&2
  echo "  Route writes through Store::with_immediate_tx / with_immediate_tx_mapped;" >&2
  echo "  route multi-statement reads through Store::with_deferred_read." >&2
  failed=1
fi

# 2. No module outside store/ touches the raw Store::conn field. The regex
#    matches `.conn` only when it is a whole token (so `.connectors` /
#    `.connector` do not false-positive).
conn_offenders=$(grep -rnE "\.conn([^A-Za-z0-9_]|$)" "$src" --include='*.rs' \
  | grep -v "^$src/store/" || true)
if [ -n "$conn_offenders" ]; then
  echo "FAIL: raw Store::conn access outside store/:" >&2
  echo "$conn_offenders" >&2
  echo "  Add a Store method, or (in #[cfg(test)] code) use" >&2
  echo "  test_hooks::with_conn_for_test." >&2
  failed=1
fi

# 3. store/ never imports a channel adapter.
tg_offenders=$(grep -rn "crate::telegram::" "$src/store" --include='*.rs' || true)
if [ -n "$tg_offenders" ]; then
  echo "FAIL: store/ imports crate::telegram:: (trust-boundary inversion):" >&2
  echo "$tg_offenders" >&2
  echo "  Carry owner verification via OwnerVerifiedProof and owner addressing" >&2
  echo "  via OwnerSurfaceRef; resolve channel addresses in the adapter layer." >&2
  failed=1
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "check-store-encapsulation: conn is encapsulated; the combinators own every"
echo "store transaction; store/ is free of channel-adapter imports."
