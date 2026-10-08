#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/rolled-route-ownership"
TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_absent '\b(sorry|admit|axiom|native_decide|unsafe)\b' \
  "rolled-route trusted proof declarations" "$PROOFS"/*.lean
if [[ "$FAILED" -ne 0 ]]; then
  echo 'Untrusted declaration or proof bypass in supplemental proofs' >&2
  exit 1
fi
(cd "$ROOT/proofs/lean" && lake +"$TOOLCHAIN" build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment SendContinuation SendEntry NestedVisit SharedSuffix; do
  LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    lean +"$TOOLCHAIN" "$PROOFS/$proof.lean" > "$EVIDENCE/$proof.log" 2>&1
done
z3 "$PROOFS/Admission.smt2" > "$EVIDENCE/z3.log"
awk '
  /^sat$/ { sat++ }
  /^unsat$/ { unsat++ }
  /^unknown$|\(error/ { failed=1 }
  END { if (failed || sat != 16 || unsat != 10) exit 1 }
' "$EVIDENCE/z3.log"
z3 "$PROOFS/SendContinuation.smt2" > "$EVIDENCE/send-continuation-z3.log"
awk '
  /^sat$/ { sat++ }
  /^unsat$/ { unsat++ }
  /^unknown$|\(error/ { failed=1 }
  END { if (failed || sat != 3 || unsat != 2) exit 1 }
' "$EVIDENCE/send-continuation-z3.log"
z3 "$PROOFS/SendEntry.smt2" > "$EVIDENCE/send-entry-z3.log"
awk '
  BEGIN { split("sat unsat sat unsat sat", expected, " ") }
  { if ($0 != expected[NR]) failed=1 }
  END { if (failed || NR != 5) exit 1 }
' "$EVIDENCE/send-entry-z3.log"
z3 "$PROOFS/NestedVisit.smt2" > "$EVIDENCE/nested-visit-z3.log"
awk '
  BEGIN { split("sat unsat sat unsat sat unsat sat unsat", expected, " ") }
  { if ($0 != expected[NR]) failed=1 }
  END { if (failed || NR != 8) exit 1 }
' "$EVIDENCE/nested-visit-z3.log"
z3 "$PROOFS/SharedSuffix.smt2" > "$EVIDENCE/shared-suffix-z3.log"
awk '
  BEGIN { split("sat unsat sat unsat sat unsat sat unsat sat sat sat sat unsat sat unsat", expected, " ") }
  { if ($0 != expected[NR]) failed=1 }
  END { if (failed || NR != 15) exit 1 }
' "$EVIDENCE/shared-suffix-z3.log"
printf 'Rolled-route proofs passed: Lean 4.30.0, 10 files; Z3 24 UNSAT obligations, 35 SAT premises/witnesses. Logs: %s\n' "$EVIDENCE"
