#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/nested-resolver-reentry"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-nested-resolver.XXXXXX)}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'nested resolver trusted proof declarations' "$PROOFS/Check.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/Check.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 6
printf '%s\n' 'nested-resolver-reentry: Lean checks passed'
