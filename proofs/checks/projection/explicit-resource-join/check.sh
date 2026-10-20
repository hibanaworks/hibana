#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" source "$ROOT/proofs/checks/projection/explicit-resource-join/Join.lean"
(cd "$ROOT/proofs/lean" && lake build Hibana.GlobalSemantics)
LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" lean "$ROOT/proofs/checks/projection/explicit-resource-join/Join.lean"
bash "$ROOT/.github/scripts/check_smt2.sh" "$ROOT/proofs/checks/projection/explicit-resource-join/Join.smt2"
(cd "$ROOT" && cargo +"${TOOLCHAIN:-stable}" test --locked --test explicit_resource_join)
