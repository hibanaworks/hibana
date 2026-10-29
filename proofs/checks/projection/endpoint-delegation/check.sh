#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
export TOOLCHAIN="${TOOLCHAIN:-stable}"
PROOFS="$ROOT/proofs/checks/projection/endpoint-delegation"
MODEL="$ROOT/proofs/lean/Hibana/EndpointOwnership.lean"
MODEL_PATH="$ROOT/proofs/lean/.lake/build/lib/lean"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-delegation-projection.XXXXXX)}
mkdir -p "$EVIDENCE"
EVIDENCE=$(cd "$EVIDENCE" && pwd)
cd "$ROOT"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
# The standalone model has its own tests. Core-only repository fixtures must
# not be enabled in this independently compiled package.
source "$ROOT/.github/scripts/repo_rustflags.sh"
if ! (
  hibana_enable_repo_tests_cfg
  export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
  export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
  export CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true
  export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true
  cargo +"$TOOLCHAIN" test --locked --release --lib \
    global::const_dsl::receive_lane_causality::actors::exhaustive \
    -- --include-ignored --nocapture --test-threads=1
) > "$EVIDENCE/exhaustive-causality.log" 2>&1; then
  cat "$EVIDENCE/exhaustive-causality.log" >&2
  exit 1
fi
bash "$ROOT/.github/scripts/lib/test_result_tool.sh" check \
  "$EVIDENCE/exhaustive-causality.log" 'endpoint delegation causality' 0
for result in \
  'exhaustive parallel queries checked=16777216' \
  'exhaustive route queries checked=33554432' \
  'exhaustive point/witness queries checked=134217728' \
  'bulk/pointwise causal comparisons checked=7873200' \
  '; 0 ignored;'; do
  if ! [[ $(rg --fixed-strings --count -- "$result" "$EVIDENCE/exhaustive-causality.log") -eq 1 ]]; then
    printf 'Missing unique causal-query result: %s\n' "$result" >&2
    cat "$EVIDENCE/exhaustive-causality.log" >&2
    exit 1
  fi
done
hibana_disable_repo_tests_cfg
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'endpoint delegation projection' "$MODEL" "$PROOFS/Claims.lean" "$PROOFS/Knowledge.lean" "$PROOFS/Issuance.lean" "$PROOFS/Causality.lean" "$PROOFS/ReceiptWait.lean" "$PROOFS/OrderFilter.lean" "$PROOFS/TransferIndex.lean" "$PROOFS/OwnerJoin.lean"
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
(
  cd "$ROOT/proofs/lean"
  lake build Hibana.EndpointOwnership
) > "$EVIDENCE/lean-model-build.log" 2>&1
LEAN_PATH="$MODEL_PATH" lean "$PROOFS/Claims.lean" > "$EVIDENCE/lean-model.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-model.log" 14
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean -o "$EVIDENCE/Knowledge.olean" "$PROOFS/Knowledge.lean" > "$EVIDENCE/lean-knowledge.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-knowledge.log" 3
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean -o "$EVIDENCE/Causality.olean" "$PROOFS/Causality.lean" > "$EVIDENCE/lean-causality.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-causality.log" 7
lean -o "$EVIDENCE/Issuance.olean" "$PROOFS/Issuance.lean" > "$EVIDENCE/lean-issuance.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-issuance.log" 4
lean -j1 "$PROOFS/ReceiptWait.lean" > "$EVIDENCE/lean-receipt-wait.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-receipt-wait.log" 3
lean -j1 "$PROOFS/OrderFilter.lean" > "$EVIDENCE/lean-order-filter.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-order-filter.log" 2
lean -j1 "$PROOFS/TransferIndex.lean" > "$EVIDENCE/lean-transfer-index.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-transfer-index.log" 4
lean -j1 "$PROOFS/OwnerJoin.lean" > "$EVIDENCE/lean-owner-join.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-owner-join.log" 3
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean "$EVIDENCE/Generated.lean" > "$EVIDENCE/lean-generated.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-generated.log" 193
model_count=0
for model in "$PROOFS"/*.smt2; do
  name="${model##*/}"
  bash "$ROOT/.github/scripts/check_smt2.sh" "$model" > "$EVIDENCE/z3-${name%.smt2}.log"
  model_count=$((model_count + 1))
done
printf 'Endpoint delegation: all %s SMT models passed EXPECT checks; per-model logs: %s/z3-*.log\n' "$model_count" "$EVIDENCE"
printf 'Endpoint ownership projection: Rust tests passed; 17 projection/knowledge, 4 issuance, 7 causality/source-order, 3 receipt-wait, 2 order-filter, 4 transfer-index and 3 owner-join general Lean theorems; 193 generated. Evidence: %s\n' "$EVIDENCE"
