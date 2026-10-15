#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT/proofs/lean"
lake build Hibana.GlobalSemantics
lake env lean ../roll-continuation-dependency/Check.lean
cd "$ROOT"
output=$(mktemp)
trap 'rm -f "$output"' EXIT
z3 proofs/roll-continuation-dependency/Check.smt2 > "$output"
cat "$output"
bash .github/scripts/lib/supplemental_proofs_tool.sh smt-sequence "$output" sat unsat sat unsat sat unsat
