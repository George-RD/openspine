#!/usr/bin/env bash
# Deliberate-negative test for the effect-write placement net, invariant #4 of
# check-store-encapsulation.sh (ticket #262). Proves the net REJECTS an
# un-audited production INSERT into an effect table (including lowercase /
# multi-space bypass attempts) and ACCEPTS the audit-paired allowlisted modules
# and cfg(test) `*tests.rs` fixtures. Exercised against throwaway fixture trees
# via OPENSPINE_EFFECT_WRITE_SRC so it never depends on the live kernel source.
set -euo pipefail

cd "$(dirname "$0")/.."
gate="$PWD/scripts/check-store-encapsulation.sh"
marker="(ticket #262)"
fails=0

ok_dir=$(mktemp -d)
bad_dir=$(mktemp -d)
trap 'rm -rf "$ok_dir" "$bad_dir"' EXIT

run_gate() { # $1 = fixture src root; sets gate_out / gate_rc
  gate_out=$(OPENSPINE_EFFECT_WRITE_SRC="$1" bash "$gate" 2>&1) && gate_rc=0 || gate_rc=$?
}

# --- ACCEPT: an audit-paired allowlisted module and a cfg(test) *tests.rs file.
mkdir -p "$ok_dir/store" "$ok_dir/failure_surfacing"
cat >"$ok_dir/store/identity.rs" <<'RS'
// allowlisted audit-paired module (routes through with_audited_effect)
tx.execute("INSERT INTO identities (id, identity_json) VALUES (?1, ?2)", params![])?;
RS
cat >"$ok_dir/failure_surfacing/tests.rs" <<'RS'
// the one known cfg(test) fixture, allowlisted by exact path
conn.execute("INSERT INTO principals (id) VALUES (?1)", params![])?;
RS
run_gate "$ok_dir"
if [ "$gate_rc" -ne 0 ]; then
  echo "FAIL: gate rejected an allowlisted / cfg(test) placement (rc=$gate_rc):" >&2
  echo "$gate_out" >&2
  fails=1
else
  echo "ok: allowlisted store module + the named cfg(test) fixture accepted"
fi

# --- REJECT: a stray un-audited insert in a non-allowlisted module (lowercase,
#     multi-space spelling proves the match is not bypassable) AND a look-alike
#     `*_tests.rs` name that is NOT the named fixture (proves the exact allowlist
#     closes the filename-heuristic bypass a production `mod rogue_tests;` opens).
mkdir -p "$bad_dir/pipeline"
cat >"$bad_dir/pipeline/rogue.rs" <<'RS'
// un-audited effect write that must be caught by the placement net
conn.execute("insert   into   identities (id) VALUES (?1)", params![])?;
RS
cat >"$bad_dir/pipeline/rogue_tests.rs" <<'RS'
// a *_tests.rs name that is not the allowlisted fixture must NOT be exempt
conn.execute("INSERT INTO principals (id) VALUES (?1)", params![])?;
RS
run_gate "$bad_dir"
if [ "$gate_rc" -eq 0 ]; then
  echo "FAIL: gate accepted an un-audited effect-table write (should reject)" >&2
  fails=1
elif ! printf '%s' "$gate_out" | grep -qF "$marker"; then
  echo "FAIL: gate rejected the write but without the #262 placement message:" >&2
  echo "$gate_out" >&2
  fails=1
elif ! printf '%s' "$gate_out" | grep -qF "rogue_tests.rs"; then
  echo "FAIL: a non-allowlisted *_tests.rs effect write slipped past the net:" >&2
  echo "$gate_out" >&2
  fails=1
else
  echo "ok: un-audited effect write and look-alike *_tests.rs both rejected"
fi

if [ "$fails" -ne 0 ]; then
  echo "check-store-encapsulation.test: FAILED" >&2
  exit 1
fi

echo "check-store-encapsulation.test: effect-write placement net accepts the"
echo "audit-paired and cfg(test) placements and rejects un-audited effect writes."
