#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/live-descendant-preview"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-live-descendant-preview.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/hygiene_common.sh"
FAILED=0
check_absent '\b(sorry|admit|axiom|native_decide|unsafe)\b' \
  'live descendant preview trusted proof declarations' "$PROOFS/LiveDescendantPreview.lean"
[[ "$FAILED" -eq 0 ]]
expected_sources=(
  src/endpoint/kernel/core/route_commit_helpers.rs
  src/endpoint/kernel/core/route_preview.rs
  src/global/typestate/cursor/scope_route.rs
  tests/rolled_publication_exit.rs
  proofs/live-descendant-preview/LiveDescendantPreview.lean
  proofs/live-descendant-preview/LiveDescendantPreview.smt2
)
[[ "$(awk '{print $2}' "$PROOFS/source.sha256")" == "$(printf '%s\n' "${expected_sources[@]}")" ]]
shasum -a 256 -c "$PROOFS/source.sha256" > "$EVIDENCE/source.log"
lean +leanprover/lean4:v4.30.0 "$PROOFS/LiveDescendantPreview.lean" > "$EVIDENCE/lean.log" 2>&1
awk '
  /does not depend on any axioms$/ { checked++; next }
  /depends on axioms: / {
    checked++; sub(/^.*depends on axioms: /, "")
    gsub(/propext|Quot.sound|\[|\]|,|[[:space:]]/, "")
    if (length($0)) failed=1
    next
  }
  NF { failed=1 }
  END { if (failed || checked != 8) exit 1 }
' "$EVIDENCE/lean.log"
z3 "$PROOFS/LiveDescendantPreview.smt2" > "$EVIDENCE/z3.log"
awk '
  /^unsat$/ { unsat++; next }
  /^sat$/ { sat++; next }
  NF { failed=1 }
  END { if (failed || unsat != 4 || sat != 1) exit 1 }
' "$EVIDENCE/z3.log"
printf 'Live descendant preview: 8 Lean theorems; Z3 4 UNSAT and 1 historical SAT witness. Evidence: %s\n' "$EVIDENCE"
