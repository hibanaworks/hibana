#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export TOOLCHAIN="${TOOLCHAIN:-stable}"
PROOFS="$ROOT/proofs/endpoint-delegation"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-delegation-projection.XXXXXX)}
mkdir -p "$EVIDENCE"
EVIDENCE=$(cd "$EVIDENCE" && pwd)
cd "$ROOT"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'endpoint delegation projection' "$PROOFS/Projection.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
cargo +"$TOOLCHAIN" fmt --manifest-path "$PROOFS/compiler/Cargo.toml" --check
cargo +"$TOOLCHAIN" test --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib > "$EVIDENCE/rust-tests.log" 2>&1
cargo +"$TOOLCHAIN" clippy --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --all-targets -- -D warnings > "$EVIDENCE/clippy.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features > "$EVIDENCE/no-std-host.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features --target thumbv6m-none-eabi > "$EVIDENCE/no-std-thumb.log" 2>&1
cargo +"$TOOLCHAIN" run --manifest-path "$PROOFS/compiler/Cargo.toml" --locked -- "$EVIDENCE/Generated.lean" > "$EVIDENCE/export.log" 2>&1
check_trusted_proof_sources 'generated endpoint delegation projection' "$EVIDENCE/Generated.lean"
[[ "$FAILED" -eq 0 ]]
lean -o "$EVIDENCE/Projection.olean" "$PROOFS/Projection.lean" > "$EVIDENCE/lean-model.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-model.log" 14
LEAN_PATH="$EVIDENCE" lean "$EVIDENCE/Generated.lean" > "$EVIDENCE/lean-generated.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-generated.log" 129
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/Ownership.smt2" > "$EVIDENCE/z3.log"
printf 'Endpoint ownership projection: Rust tests passed; 14 general and 129 generated Lean theorems; Z3 7 UNSAT, 2 counterexample SAT, 1 positive SAT. Evidence: %s\n' "$EVIDENCE"
