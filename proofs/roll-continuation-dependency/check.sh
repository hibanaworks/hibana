#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources "roll-continuation-dependency trusted proof declarations" "$ROOT/proofs/roll-continuation-dependency/Check.lean"
if [[ "$FAILED" -ne 0 ]]; then
  exit 1
fi
cd "$ROOT/proofs/lean"
lake build Hibana.GlobalSemantics
lake env lean ../roll-continuation-dependency/Check.lean
cd "$ROOT"
output=$(mktemp)
trap 'rm -f "$output"' EXIT
z3 proofs/roll-continuation-dependency/Check.smt2 > "$output"
cat "$output"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$output" sat unsat sat unsat sat unsat
