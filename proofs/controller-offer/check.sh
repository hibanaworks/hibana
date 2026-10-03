#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/controller-offer"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-controller-offer-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/hygiene_common.sh"
FAILED=0
check_absent '\b(sorry|admit|axiom|native_decide|unsafe)\b' \
  'controller-offer trusted proof declarations' "$PROOFS/ControllerOffer.lean"
[[ "$FAILED" -eq 0 ]]
expected_sources=(
  src/endpoint/kernel/core/offer_refresh.rs
  src/endpoint/kernel/core/decision_resolver/impls/select.rs
  src/endpoint/kernel/core/decision_resolver/impls/send.rs
  src/endpoint/kernel/offer.rs
  src/endpoint/kernel/offer/commit.rs
  src/endpoint/kernel/offer/select_alignment.rs
  src/global/typestate/cursor/scope_route/navigation.rs
  tests/nested_resolver_self_continuations.rs
  proofs/controller-offer/ControllerOffer.lean
  proofs/controller-offer/ControllerOffer.smt2
)
if [[ "$(awk '{print $2}' "$PROOFS/source.sha256")" != "$(printf '%s\n' "${expected_sources[@]}")" ]]; then
  echo 'Controller-offer source inventory changed' >&2
  exit 1
fi
shasum -a 256 -c "$PROOFS/source.sha256" > "$EVIDENCE/source.log"
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
