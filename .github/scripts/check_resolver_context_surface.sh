#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if rg -n "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(eff_index|scope_id|scope_trace)[[:space:]]*\\(" src/control/cluster/core.rs; then
  echo "boundary violation: ResolverContext must not expose internal coordinate getters" >&2
  FAILED=1
fi

if rg -n "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(scope_id|scope_kind|scope_region)[[:space:]]*\\(" src/endpoint/cursor.rs; then
  echo "boundary violation: RouteBranch must not expose scope coordinate helpers" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver/route surface boundary check passed"
