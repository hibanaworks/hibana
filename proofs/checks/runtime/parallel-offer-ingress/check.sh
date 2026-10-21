#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
cd "$(dirname "$0")/../../../.."
ingress_evidence=${1:-$(mktemp -d /tmp/hibana-parallel-ingress-proof.XXXXXX)}
mkdir -p "$ingress_evidence"
lean --version | bash "$(cd "$(dirname "$0")/../../../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean proofs/checks/runtime/parallel-offer-ingress/Ingress.lean 2>&1 | tee "$ingress_evidence/lean.log"
# Inspect the actual kernel axiom inventory, not source spellings or paths.
bash .github/scripts/lib/supplemental_proofs_tool.sh lean "$ingress_evidence/lean.log" 18
z3 proofs/checks/runtime/parallel-offer-ingress/Ingress.smt2 | tee "$ingress_evidence/z3.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$ingress_evidence/z3.log" sat sat unsat unsat unsat unsat unsat unsat unsat unsat sat unsat unsat unsat
printf 'Parallel-offer ingress: Lean 18 theorems; Z3 11 UNSAT obligations, 3 SAT premises. Evidence: %s\n' "$ingress_evidence"
