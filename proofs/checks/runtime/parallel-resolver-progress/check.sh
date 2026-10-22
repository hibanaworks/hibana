#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources "parallel-resolver-progress trusted proof declarations" "$ROOT/proofs/checks/runtime/parallel-resolver-progress/Check.lean"
if [[ "$FAILED" -ne 0 ]]; then
  exit 1
fi
cd "$ROOT/proofs/lean"
lake build Hibana.GlobalSemantics
lake env lean "$ROOT/proofs/checks/runtime/parallel-resolver-progress/Check.lean"
cd "$ROOT"
output=$(mktemp)
trap 'rm -f "$output"' EXIT
z3 proofs/checks/runtime/parallel-resolver-progress/Check.smt2 > "$output"
cat "$output"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$output" \
  sat unsat sat unsat sat unsat sat sat unsat sat unsat sat unsat sat unsat \
  sat unsat sat unsat unsat \
  sat unsat sat unsat unsat unsat unsat unsat sat unsat sat unsat unsat \
  sat unsat unsat unsat unsat
