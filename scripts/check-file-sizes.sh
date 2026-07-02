#!/usr/bin/env bash
# Fails if any crates/**/*.rs file exceeds 500 lines, unless its first
# non-blank line is `// openspine:allow-large-module reason: <reason>`.
set -euo pipefail

cd "$(dirname "$0")/.."

limit=500
failed=0

while IFS= read -r -d '' file; do
  lines=$(wc -l < "$file" | tr -d ' ')
  if [ "$lines" -le "$limit" ]; then
    continue
  fi

  first_non_blank=$(grep -m1 -v '^[[:space:]]*$' "$file" || true)
  if [[ "$first_non_blank" == "// openspine:allow-large-module reason:"* ]]; then
    continue
  fi

  echo "FAIL: $file has $lines lines (limit $limit) and no allow-large-module escape hatch" >&2
  failed=1
done < <(find crates -name '*.rs' -type f -print0)

if [ "$failed" -ne 0 ]; then
  echo "" >&2
  echo "Split the file(s) above, or add as the first non-blank line:" >&2
  echo "  // openspine:allow-large-module reason: <why this file must stay large>" >&2
  exit 1
fi

echo "check-file-sizes: all crates/**/*.rs files are within the ${limit}-line limit."
