#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

check_absent "\\bResolverContext\\b" \
  "ResolverContext must not be a public resolver argument" \
  src/session/cluster/core.rs src/session/cluster/core src/runtime README.md

check_absent "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(scope_id|scope_kind|scope_region)[[:space:]]*\\(" \
  "RouteBranch must not expose scope coordinate helpers" \
  src/endpoint/kernel/core.rs src/endpoint/kernel/core

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver/route surface boundary check passed"
