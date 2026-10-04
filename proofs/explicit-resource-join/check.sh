#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
if [[ "$(lean --version)" != *"version 4.30.0"* ]]; then
  echo 'Explicit join validation requires Lean 4.30.0' >&2
  exit 1
fi
if grep -En '\b(sorry|admit|axiom|native_decide|unsafe)\b' "$ROOT/proofs/explicit-resource-join/Join.lean"; then
  echo 'Untrusted declaration in explicit join proof' >&2
  exit 1
fi
(cd "$ROOT/proofs/lean" && lake build Hibana.GlobalSemantics)
LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" lean "$ROOT/proofs/explicit-resource-join/Join.lean"
python3 "$ROOT/proofs/explicit-resource-join/check_join.py"
(cd "$ROOT" && cargo test --locked --test explicit_resource_join)
