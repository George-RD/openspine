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
#   4. Every `INSERT INTO` an effect table (pending_draft_writes / identities /
#      principals) appears ONLY in an EXACT file allowlist: the store modules
#      that route through the audit-paired combinators (`with_audited_effect` /
#      `begin_effect` / `settle_effect`), plus the one known cfg(test) fixture
#      that seeds an effect row. Any other placement — including a look-alike
#      `*_tests.rs` name — fails CI (ticket #262). This bounds PLACEMENT, not
#      pairing: Rust cannot forbid a raw `tx.execute` SQL string, so the net
#      does not prove the pairing at compile time — the with_audited_effect
#      signature and routing do.
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

# 4. Effect-table INSERTs live only in the audit-paired store modules, plus the
#    one known cfg(test) fixture (ticket #262). This is an EXACT allowlist, not
#    a filename heuristic: AGENTS.md only *prefers* the `<name>_tests.rs`
#    convention, so a production `mod rogue_tests;` could otherwise smuggle an
#    un-audited effect write past the net. A `*_tests.rs` name is therefore NOT
#    auto-exempt; the single test file that legitimately seeds an effect row is
#    named explicitly, and any new such fixture must be added here by name.
#    Placement net, NOT a pairing proof. Matching is case-insensitive with
#    flexible whitespace so lowercase / multi-space spellings cannot bypass it;
#    the explicit non-word boundary keeps `identities` from matching
#    `identity_identifiers`. The scan root is overridable via
#    OPENSPINE_EFFECT_WRITE_SRC so the invariant can be exercised against
#    fixtures (see check-store-encapsulation.test.sh).
effect_src="${OPENSPINE_EFFECT_WRITE_SRC:-$src}"
effect_offenders=$(grep -rilE \
  "insert[[:space:]]+into[[:space:]]+(pending_draft_writes|identities|principals)([^a-z0-9_]|\$)" \
  "$effect_src" --include='*.rs' \
  | grep -vE '(store/(identity|audited_effect|effect_settlement|pending_draft)|failure_surfacing/tests)\.rs$' \
  || true)
if [ -n "$effect_offenders" ]; then
  echo "FAIL: effect-table INSERT outside the audit-paired allowlist (ticket #262):" >&2
  echo "$effect_offenders" >&2
  echo "  Effect rows (pending_draft_writes / identities / principals) must be" >&2
  echo "  written only via Store::with_audited_effect / begin_effect /" >&2
  echo "  settle_effect. Put the write in store/identity.rs, store/audited_effect.rs," >&2
  echo "  store/effect_settlement.rs, or store/pending_draft.rs. A new cfg(test)" >&2
  echo "  fixture that seeds an effect row must be added to this allowlist by name." >&2
  failed=1
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "check-store-encapsulation: conn is encapsulated; the combinators own every"
echo "store transaction; store/ is free of channel-adapter imports; effect-table"
echo "writes stay in the audit-paired store modules."
