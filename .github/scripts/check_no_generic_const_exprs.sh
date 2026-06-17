#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

check_absent 'generic_const_exprs|where\s*\[\s*\(\)\s*;|[A-Za-z0-9_:]+::[A-Za-z0-9_]+::<[^>]+>\(\)\s*\]' \
  "use fixed-capacity + len instead of type-level arithmetic" \
  src tests

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no generic const expressions check passed"
