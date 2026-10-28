#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/live-descendant-preview"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-live-descendant-preview.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'live descendant preview trusted proof declarations' "$PROOFS/LiveDescendantPreview.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$(cd "$(dirname "$0")/../../../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/LiveDescendantPreview.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 14
z3 "$PROOFS/LiveDescendantPreview.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-counts "$EVIDENCE/z3.log" 2 8
printf 'Live descendant preview: 14 Lean theorems; Z3 8 UNSAT and 2 historical SAT witnesses. Evidence: %s\n' "$EVIDENCE"
