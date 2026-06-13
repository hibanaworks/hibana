#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if rg -n "\\bResolverContext\\b" src/session/cluster/core.rs src/session/cluster/core src/runtime README.md; then
  echo "boundary violation: ResolverContext must not be a public resolver argument" >&2
  FAILED=1
fi

if rg -n "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(scope_id|scope_kind|scope_region)[[:space:]]*\\(" src/endpoint/kernel/core.rs src/endpoint/kernel/core; then
  echo "boundary violation: RouteBranch must not expose scope coordinate helpers" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver/route surface boundary check passed"
