#!/usr/bin/env bash
# Fails if the generated OpenSpec OMP files (.omp/skills/openspec-*,
# .omp/commands/opsx-*) have lost this repo's hand-patched archive ceremony —
# which happens silently whenever `openspec init/update --tools oh-my-pi`
# regenerates them. Re-apply the patches (see openspec/openspine-change-sequence.md,
# "Ceremony per change") before committing regenerated output.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

require() { # file pattern description
  if ! grep -qF -- "$2" "$1"; then
    echo "FAIL: $1 is missing '$2' ($3)" >&2
    fail=1
  fi
}

forbid() { # file pattern description
  if grep -qF -- "$2" "$1"; then
    echo "FAIL: $1 contains '$2' ($3)" >&2
    fail=1
  fi
}

for f in .omp/skills/openspec-archive-change/SKILL.md .omp/commands/opsx-archive.md; do
  require "$f" 'openspec archive "<name>" --yes' "archive must use the mechanical --yes ceremony"
  require "$f" 'openspec validate --all --strict' "archive must be followed by strict validation"
  require "$f" '## MODIFIED Requirements' "pre-seeded conflicts must be fixed as MODIFIED deltas"
  forbid "$f" 'mv "<changeRoot>"' "upstream raw-move archive step (regeneration reverted the patch)"
  forbid "$f" 'hand-apply' "hand-applying deltas is retired; archive --yes applies them mechanically"
done

for f in .omp/skills/openspec-apply-change/SKILL.md .omp/commands/opsx-apply.md \
  .omp/skills/openspec-archive-change/SKILL.md .omp/commands/opsx-archive.md; do
  forbid "$f" 'openspec-continue-change' "reference to a skill that is not generated in this repo"
  forbid "$f" 'opsx-continue' "reference to a command that is not generated in this repo"
  forbid "$f" 'openspec-sync-specs' "reference to a skill that is not generated in this repo"
done

if [ "$fail" -ne 0 ]; then
  echo "check-omp-ceremony: generated OMP files drifted from the repo archive ceremony." >&2
  exit 1
fi

echo "check-omp-ceremony: generated OMP files carry the repo archive ceremony."
