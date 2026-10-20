#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/offer-prefix-dependency"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-offer-prefix.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'offer prefix dependency trusted declarations' "$PROOFS/Check.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/Check.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 24
z3 "$PROOFS/Check.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/z3.log" unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat unsat sat sat unsat unsat unsat unsat unsat unsat sat
printf 'Offer prefix dependency: 24 Lean theorems; Z3 23 UNSAT and 3 historical SAT. Evidence: %s\n' "$EVIDENCE"
