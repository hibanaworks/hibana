#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
cd "$(dirname "$0")/../../../.."
ingress_evidence=${1:-$(mktemp -d /tmp/hibana-parallel-ingress-proof.XXXXXX)}
mkdir -p "$ingress_evidence"
lean --version | bash "$(cd "$(dirname "$0")/../../../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean proofs/checks/runtime/parallel-offer-ingress/Ingress.lean 2>&1 | tee "$ingress_evidence/lean.log"
# Inspect the actual kernel axiom inventory, not source spellings or paths.
bash .github/scripts/lib/supplemental_proofs_tool.sh lean "$ingress_evidence/lean.log" 21
printf '%s\n' 'parallel-offer-ingress: Lean checks passed'
