#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
cd "$(dirname "$0")/../../../.."
evidence=${1:-target/tap-commit-proof}
mkdir -p "$evidence"
lean proofs/checks/runtime/tap-commit/Encoding.lean 2>&1 | tee "$evidence/lean.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh lean "$evidence/lean.log" 9
z3 proofs/checks/runtime/tap-commit/Encoding.smt2 | tee "$evidence/z3.log"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$evidence/z3.log" sat unsat sat unsat sat
printf 'Tap commit encoding: 9 Lean theorems; exact bit-field recovery and injectivity; truncated-source control aliases.\n'
