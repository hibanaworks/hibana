#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
ingress_evidence=${1:-$(mktemp -d /tmp/hibana-parallel-ingress-proof.XXXXXX)}
mkdir -p "$ingress_evidence"
lean +leanprover/lean4:v4.30.0 proofs/parallel-offer-ingress/Ingress.lean > "$ingress_evidence/lean.log" 2>&1
# Inspect the actual kernel axiom inventory, not source spellings or paths.
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
  END { if (failed || checked != 14) exit 1 }
' "$ingress_evidence/lean.log"
z3 proofs/parallel-offer-ingress/Ingress.smt2 > "$ingress_evidence/z3.log"
awk '
  BEGIN { split("sat sat unsat unsat unsat unsat unsat unsat unsat unsat", expected, " ") }
  { if ($0 != expected[NR]) failed=1 }
  END { if (failed || NR != 10) exit 1 }
' "$ingress_evidence/z3.log"
printf 'Parallel-offer ingress: Lean 14 theorems; Z3 8 UNSAT obligations, 2 SAT premises. Evidence: %s\n' "$ingress_evidence"
