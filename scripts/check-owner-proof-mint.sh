#!/usr/bin/env bash
# Owner-proof mint gate (identity.rs D-005/D-006; ticket #263, store-review
# Finding B). Permissions promise; Auditor + Lyra.
#
# `OwnerVerifiedProof` is the unforgeable, zero-authority capability token a
# channel adapter mints AFTER completing its own owner verification, so that
# owner-gated kernel code depends on the neutral identity seam instead of
# reaching up into a connector adapter. Its production constructor is
# `OwnerVerifiedProof::mint()` (pub(crate)); the identity.rs docstring promises
# only channel adapters mint it. This gate turns that convention into a CI
# invariant, the same grep-checkable posture #218 established for the store:
#
#   Any production `OwnerVerifiedProof::mint(` call MUST live in a channel-
#   adapter module on the allowlist below. Tests mint via the #[cfg(test)]
#   `OwnerVerifiedProof::test_new()`, which is a distinct method and stays
#   unrestricted.
#
# The token `OwnerVerifiedProof::mint(` matches only call sites: it does not
# match the definition (`fn mint(`) nor the docstring reference
# (`[`OwnerVerifiedProof::mint`]`, no paren), so identity.rs is not an offender.
set -euo pipefail

cd "$(dirname "$0")/.."

src="crates/openspine-kernel/src"

# Channel-adapter modules allowed to mint the owner-verified proof in
# production. Extend this list as connectors land — each new adapter that
# performs its own owner verification before handing an event to the kernel.
adapter_allowlist=(
  "$src/telegram.rs"
)

offenders=""
while IFS= read -r file; do
  [ -z "$file" ] && continue
  allowed=0
  for allow in "${adapter_allowlist[@]}"; do
    if [ "$file" = "$allow" ]; then
      allowed=1
      break
    fi
  done
  if [ "$allowed" -eq 0 ]; then
    offenders+="$file"$'\n'
  fi
done < <(grep -rln "OwnerVerifiedProof::mint(" "$src" --include='*.rs' || true)

if [ -n "$offenders" ]; then
  echo "FAIL: OwnerVerifiedProof::mint() outside the channel-adapter allowlist:" >&2
  printf '%s' "$offenders" | sed 's/^/  /' >&2
  echo "  Only channel adapters mint OwnerVerifiedProof (identity.rs D-005/D-006)." >&2
  echo "  Put the production mint in an adapter module and add that module to the" >&2
  echo "  allowlist in scripts/check-owner-proof-mint.sh. In #[cfg(test)] code," >&2
  echo "  mint via OwnerVerifiedProof::test_new() instead." >&2
  exit 1
fi

echo "check-owner-proof-mint: OwnerVerifiedProof::mint() is confined to the"
echo "channel-adapter allowlist; tests mint via test_new()."
