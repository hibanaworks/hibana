#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
cd "$(dirname "$0")/../../../.."
evidence=${1:-target/tap-commit-proof}
mkdir -p "$evidence"
lean proofs/checks/runtime/tap-commit/Encoding.lean 2>&1 | tee "$evidence/lean.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh lean "$evidence/lean.log" 13
z3 proofs/checks/runtime/tap-commit/Encoding.smt2 | tee "$evidence/z3.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$evidence/z3.log" sat unsat sat unsat sat unsat sat sat unsat sat
printf 'Tap commit and reentry encoding: 13 Lean theorems; exact field recovery and injectivity; truncated-source and scope controls alias.\n'
