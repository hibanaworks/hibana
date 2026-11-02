#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/parked-offer"
EVIDENCE=${1:-"$ROOT/target/parked-receive-proof"}
mkdir -p "$EVIDENCE"
cd "$ROOT"
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'parked receive lane exclusion' "$PROOFS/Receive.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$PROOFS/Receive.lean" > "$EVIDENCE/lean.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean.log" 9
printf '%s\n' 'parked-offer: Lean checks passed'
