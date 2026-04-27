#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if rg -n 'generic_const_exprs|where\s*\[\s*\(\)\s*;|[A-Za-z0-9_:]+::[A-Za-z0-9_]+::<[^>]+>\(\)\s*\]' src tests examples integration internal 2>/dev/null; then
  echo "generic const expression hygiene violation: use fixed-capacity + len instead of type-level arithmetic" >&2
  exit 1
fi

echo "no generic const expressions check passed"
