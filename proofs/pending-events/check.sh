#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
scan_evidence=${1:-$(mktemp -d /tmp/hibana-pending-proof.XXXXXX)}
mkdir -p "$scan_evidence"
lean +leanprover/lean4:v4.30.0 proofs/pending-events/Scan.lean > "$scan_evidence/lean.log" 2>&1
cat "$scan_evidence/lean.log"
# Check the produced proof boundary, rather than linting source spellings/paths.
if rg 'sorryAx|_native|error:|warning:' "$scan_evidence/lean.log"; then exit 1; fi
z3 proofs/pending-events/Scan.smt2 > "$scan_evidence/z3.log"
awk '
  BEGIN { split("sat unsat sat unsat unsat unsat", expected, " ") }
  { if ($0 != expected[NR]) failed=1 }
  END { if (failed || NR != 6) exit 1 }
' "$scan_evidence/z3.log"
printf 'Pending-event scan: Lean 7 theorems; Z3 4 UNSAT obligations, 2 SAT premises. Evidence: %s\n' "$scan_evidence"
