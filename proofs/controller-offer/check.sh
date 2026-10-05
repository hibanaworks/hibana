#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/controller-offer"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-controller-offer-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_absent '\b(sorry|admit|axiom|native_decide|unsafe)\b' \
  'controller-offer trusted proof declarations' "$PROOFS/ControllerOffer.lean"
[[ "$FAILED" -eq 0 ]]
lean +leanprover/lean4:v4.30.0 "$PROOFS/ControllerOffer.lean" > "$EVIDENCE/lean.log" 2>&1
awk '
  /does not depend on any axioms$/ { checked++; next }
  /depends on axioms: / {
    checked++
    sub(/^.*depends on axioms: /, "")
    gsub(/propext|Quot.sound|\[|\]|,|[[:space:]]/, "")
    if (length($0)) failed=1
    next
  }
  NF { failed=1 }
  END { if (failed || checked != 13) exit 1 }
' "$EVIDENCE/lean.log"
z3 "$PROOFS/ControllerOffer.smt2" > "$EVIDENCE/z3.log"
awk '
  /^sat$/ { sat++; next }
  /^unsat$/ { unsat++; next }
  NF { failed=1 }
  END { if (failed || sat != 2 || unsat != 9) exit 1 }
' "$EVIDENCE/z3.log"
printf 'Controller-offer proofs passed: 13 Lean kernel theorems; Z3 9 UNSAT obligations and 2 historical SAT witnesses. Evidence: %s\n' "$EVIDENCE"
