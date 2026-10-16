#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/controller-offer"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-controller-offer-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'controller-offer trusted proof declarations' "$PROOFS/ControllerOffer.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$(cd "$(dirname "$0")/../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/ControllerOffer.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 25
z3 "$PROOFS/ControllerOffer.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-counts "$EVIDENCE/z3.log" 4 20
printf 'Controller-offer proofs passed: 25 Lean kernel theorems; Z3 20 UNSAT obligations and 4 SAT witnesses. Evidence: %s\n' "$EVIDENCE"
