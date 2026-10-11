#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
cd "$(dirname "$0")/../.."
scan_evidence=${1:-$(mktemp -d /tmp/hibana-pending-proof.XXXXXX)}
mkdir -p "$scan_evidence"
lean --version | bash "$(cd "$(dirname "$0")/../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean proofs/pending-events/Scan.lean 2>&1 | tee "$scan_evidence/lean.log"
# Check the produced proof boundary, rather than linting source spellings/paths.
bash .github/scripts/lib/supplemental_proofs_tool.sh lean-clean "$scan_evidence/lean.log"
z3 proofs/pending-events/Scan.smt2 | tee "$scan_evidence/z3.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$scan_evidence/z3.log" sat unsat sat unsat unsat unsat
printf 'Pending-event scan: Lean 7 theorems; Z3 4 UNSAT obligations, 2 SAT premises. Evidence: %s\n' "$scan_evidence"
