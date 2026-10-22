#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/rolled-route-ownership"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources "rolled-route trusted proof declarations" "$PROOFS"/*.lean
if [[ "$FAILED" -ne 0 ]]; then
  echo 'Untrusted declaration or proof bypass in supplemental proofs' >&2
  exit 1
fi
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
(cd "$ROOT/proofs/lean" && lake build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment SendContinuation SendEntry NestedVisit SharedSuffix; do
  LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    lean "$PROOFS/$proof.lean" > "$EVIDENCE/$proof.log" 2>&1
done
z3 "$PROOFS/Admission.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-admission "$EVIDENCE/z3.log"
z3 "$PROOFS/SendContinuation.smt2" > "$EVIDENCE/send-continuation-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-counts "$EVIDENCE/send-continuation-z3.log" 3 2
z3 "$PROOFS/SendEntry.smt2" > "$EVIDENCE/send-entry-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/send-entry-z3.log" sat unsat sat unsat sat
z3 "$PROOFS/NestedVisit.smt2" > "$EVIDENCE/nested-visit-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/nested-visit-z3.log" sat unsat sat unsat sat unsat sat unsat
z3 "$PROOFS/SharedSuffix.smt2" > "$EVIDENCE/shared-suffix-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/shared-suffix-z3.log" sat unsat sat unsat sat unsat sat unsat sat sat sat sat unsat sat unsat
printf 'Rolled-route proofs passed: Lean 4.30.0, 10 files; Z3 23 UNSAT obligations, 33 SAT premises/witnesses. Logs: %s\n' "$EVIDENCE"
