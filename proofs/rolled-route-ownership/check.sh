#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/rolled-route-ownership"
TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
if rg -n '\b(sorry|admit|axiom|native_decide|unsafe)\b' "$PROOFS"/*.lean; then
  echo 'Untrusted declaration or proof bypass in supplemental proofs' >&2
  exit 1
fi
(cd "$ROOT/proofs/lean" && lake +"$TOOLCHAIN" build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment; do
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
printf 'Rolled-route proofs passed: Lean 4.30.0, 6 files; Z3 10 UNSAT obligations, 16 SAT premises/witnesses. Logs: %s\n' "$EVIDENCE"
