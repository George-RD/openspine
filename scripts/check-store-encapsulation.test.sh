#!/usr/bin/env bash
# Exercise invariant #4 through the real gate in an isolated repository. Each
# fixture runs alone so a detected offender cannot hide an undetected one.
set -euo pipefail

cd "$(dirname "$0")/.."
repo="$PWD"
marker="(ticket #262)"
fails=0
cases=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Keep the unrelated Store checks hermetic. Copy the actual entrypoint rather
# than duplicating its logic; check.sh separately scans the real source tree.
mkdir -p "$tmp/repo/scripts" "$tmp/repo/crates/openspine-kernel/src/store"
cp "$repo/scripts/check-store-encapsulation.sh" "$tmp/repo/scripts/"
if [ -f "$repo/scripts/check-effect-writes.mjs" ]; then
  cp "$repo/scripts/check-effect-writes.mjs" "$tmp/repo/scripts/"
fi
: >"$tmp/repo/crates/openspine-kernel/src/store/mod.rs"
gate="$tmp/repo/scripts/check-store-encapsulation.sh"

run_gate() {
  gate_out=$(OPENSPINE_EFFECT_WRITE_SRC="$1" bash "$gate" 2>&1) && gate_rc=0 || gate_rc=$?
}

check_result() { # expected status, diagnostic path
  cases=$((cases + 1))
  if [ "$gate_rc" -ne "$1" ]; then
    echo "FAIL: $label: expected rc=$1, got $gate_rc" >&2
    echo "$gate_out" >&2
    fails=$((fails + 1))
  elif [ "$1" -ne 0 ] && { ! grep -qF "$marker" <<<"$gate_out" ||
    ! grep -qF "$2" <<<"$gate_out"; }; then
    echo "FAIL: $label: missing placement diagnostic or offending path ($2)" >&2
    echo "$gate_out" >&2
    fails=$((fails + 1))
  else
    echo "ok: $label"
  fi
}

fixture() { # expected status, source-relative path, SQL, label, optional string mode
  label="$4"
  local root="$tmp/fixture trees/$cases"
  mkdir -p "$root/$(dirname "$2")"
  if [ "${5:-raw}" = string ]; then
    printf 'tx.execute("%s", [])?;\n' "$3" >"$root/$2"
  else
    printf 'tx.execute(r##"%s"##, [])?;\n' "$3" >"$root/$2"
  fi
  # Spaces and a trailing slash must not change exact allowlist membership.
  run_gate "$root/"
  check_result "$1" "$2"
}

for path in store/identity.rs store/audited_effect.rs store/effect_settlement.rs \
  store/pending_draft.rs failure_surfacing/tests.rs; do
  fixture 0 "$path" 'INSERT INTO identities (id) VALUES (1)' "exact allowlist: $path"
done
for path in pipeline/rogue.rs pipeline/rogue_tests.rs pipeline/store/identity.rs \
  pipeline/store/audited_effect.rs pipeline/store/effect_settlement.rs \
  pipeline/store/pending_draft.rs pipeline/failure_surfacing/tests.rs \
  notstore/identity.rs notfailure_surfacing/tests.rs; do
  fixture 1 "$path" 'INSERT INTO identities (id) VALUES (1)' "untrusted path: $path"
done
for table in identities principals pending_draft_writes; do
  fixture 1 pipeline/rogue.rs "insert   into   $table (id) VALUES (1)" "lowercase: $table"
  fixture 1 pipeline/rogue.rs "$(printf 'INSERT\nINTO\n%s (id) VALUES (1)' "$table")" "multiline: $table"
  fixture 1 pipeline/rogue.rs "REPLACE INTO $table (id) VALUES (1)" "replace: $table"
done
for conflict in ROLLBACK ABORT FAIL IGNORE REPLACE; do
  fixture 1 pipeline/rogue.rs "INSERT OR $conflict INTO identities (id) VALUES (1)" "conflict: $conflict"
done
for target in '"identities"' '[identities]' '`identities`' "'identities'" \
  'main.identities' '"main" . "identities"' 'main.[principals]' \
  '[main].`pending_draft_writes`' "'main'.'identities'"; do
  fixture 1 pipeline/rogue.rs "INSERT INTO $target (id) VALUES (1)" "quoted/qualified: $target"
done
fixture 1 pipeline/rogue.rs 'INSERT/**/INTO/* row */identities (id) VALUES (1)' 'block comments'
fixture 1 pipeline/rogue.rs $'INSERT -- row\nINTO -- target\nprincipals (id) VALUES (1)' 'line comments'
fixture 1 pipeline/rogue.rs $'INSERT\r\nOR\tIGNORE\r\nINTO identities (id) VALUES (1)' 'CRLF and tabs'
fixture 1 pipeline/rogue.rs 'INSERT INTO \"identities\" (id) VALUES (1)' 'Rust escaped quotes' string
fixture 1 pipeline/rogue.rs 'INSERT\nINTO\tidentities (id) VALUES (1)' 'Rust escaped whitespace' string
fixture 1 pipeline/rogue.rs $'INSERT \\\n    INTO identities (id) VALUES (1)' 'Rust line continuation' string
fixture 1 pipeline/rogue.rs 'WITH data AS (SELECT 1) INSERT INTO identities SELECT * FROM data' 'CTE insert'
for table in identity_identifiers identities_archive principals2 pending_draft_writes_backup \
  'main.identities_archive' '"identities_archive"' 'identities$archive'; do
  fixture 0 pipeline/ordinary.rs "INSERT INTO $table (id) VALUES (1)" "non-effect table: $table"
done
fixture 0 pipeline/ordinary.rs 'SELECT id FROM identities' 'read-only SQL'
fixture 0 pipeline/ordinary.rs '' 'empty source'
fixture 0 pipeline/ordinary.rs 'REINSERT INTO identities (id) VALUES (1)' 'keyword boundary'

label='missing scan root fails closed'
run_gate "$tmp/missing"
check_result 1 "$tmp/missing"
label='file used as scan root fails closed'
: >"$tmp/not-a-directory"
run_gate "$tmp/not-a-directory"
check_result 1 "$tmp/not-a-directory"
label='unreadable source (dangling symlink) fails closed'
mkdir -p "$tmp/dangling/pipeline"
ln -s "$tmp/no-such-file" "$tmp/dangling/pipeline/rogue.rs"
run_gate "$tmp/dangling"
check_result 1 'pipeline/rogue.rs'
label='symlinked directory cannot hide source'
mkdir -p "$tmp/linked"
ln -s "$tmp/fixture trees/0" "$tmp/linked/nested"
run_gate "$tmp/linked"
check_result 1 'nested'

if [ "$fails" -ne 0 ]; then
  echo "check-store-encapsulation.test: $fails/$cases cases FAILED" >&2
  exit 1
fi
echo "check-store-encapsulation.test: $cases isolated cases passed."
