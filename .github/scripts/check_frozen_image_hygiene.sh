#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

while IFS= read -r -d '' file; do
  hits="$(
    awk '
      BEGIN { in_struct = 0 }
      /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?struct[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[^\\{]*\{/ {
        in_struct = 1
        next
      }
      in_struct && /^[[:space:]]*\}/ {
        in_struct = 0
        next
      }
      in_struct && /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?[A-Za-z_][A-Za-z0-9_]*:[[:space:]]*\*mut/ {
        print FILENAME ":" NR ":" $0
      }
    ' "${file}"
  )"
  if [[ -n "${hits}" ]]; then
    printf '%s\n' "${hits}"
    echo "frozen image hygiene violation: raw-pointer field leaked into frozen image owner" >&2
    FAILED=1
  fi
done < <(find src/global/compiled/images -name '*.rs' -print0)

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "frozen image hygiene check passed"
