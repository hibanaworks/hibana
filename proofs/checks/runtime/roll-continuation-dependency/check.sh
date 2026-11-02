#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources "roll-continuation-dependency trusted proof declarations" "$ROOT/proofs/checks/runtime/roll-continuation-dependency/Check.lean"
if [[ "$FAILED" -ne 0 ]]; then
  exit 1
fi
cd "$ROOT/proofs/lean"
lake build Hibana.GlobalSemantics
lake env lean "$ROOT/proofs/checks/runtime/roll-continuation-dependency/Check.lean"
printf '%s\n' 'roll-continuation-dependency: Lean checks passed'
