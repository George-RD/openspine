#!/usr/bin/env bash
# Fails if any `test: <name>` row in docs/threat-claims.md does not name an
# actual test in the workspace, or if the register has been gutted (zero
# `test:` rows).
set -euo pipefail

cd "$(dirname "$0")/.."

claims_doc="docs/threat-claims.md"
test_list_file="$(mktemp)"
trap 'rm -f "$test_list_file"' EXIT

cargo test --workspace -- --list >"$test_list_file" 2>&1

claim_names=$(grep -o 'test: [a-zA-Z0-9_]*' "$claims_doc" | sed 's/^test: //' | sort -u)

if [ -z "$claim_names" ]; then
  echo "FAIL: docs/threat-claims.md has zero 'test:' rows — the claims register looks gutted." >&2
  exit 1
fi

missing=""
while IFS= read -r name; do
  [ -z "$name" ] && continue
  if ! grep -qE "(::|^)${name}: test\$" "$test_list_file"; then
    missing="${missing}${name}\n"
  fi
done <<EOF
$claim_names
EOF

if [ -n "$missing" ]; then
  echo "FAIL: docs/threat-claims.md references test(s) that do not exist:" >&2
  printf '%b' "$missing" >&2
  exit 1
fi

echo "check-claims: every 'test:' row in docs/threat-claims.md names a real test."
