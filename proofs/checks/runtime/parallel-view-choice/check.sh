#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/parallel-view-choice"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-parallel-view-choice.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'parallel view choice declarations' "$PROOFS/Check.lean" "$PROOFS/LocalFork.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/Check.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 17
lean "$PROOFS/LocalFork.lean" > "$EVIDENCE/local-fork-lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/local-fork-lean.log" 5
printf '%s\n' 'parallel-view-choice: Lean checks passed'
