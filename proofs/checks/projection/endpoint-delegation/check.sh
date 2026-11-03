#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
export TOOLCHAIN="${TOOLCHAIN:-stable}"
PROOFS="$ROOT/proofs/checks/projection/endpoint-delegation"
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
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
cargo +"$TOOLCHAIN" fmt --manifest-path "$PROOFS/compiler/Cargo.toml" --check
cargo +"$TOOLCHAIN" test --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib > "$EVIDENCE/rust-tests.log" 2>&1
cargo +"$TOOLCHAIN" clippy --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --all-targets -- -D warnings > "$EVIDENCE/clippy.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features > "$EVIDENCE/no-std-host.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features --target thumbv6m-none-eabi > "$EVIDENCE/no-std-thumb.log" 2>&1
cargo +"$TOOLCHAIN" run --manifest-path "$PROOFS/compiler/Cargo.toml" --locked -- "$EVIDENCE/Generated.lean" > "$EVIDENCE/export.log" 2>&1
(
  cd "$ROOT/proofs/lean"
  lake build Hibana.EndpointOwnership
) > "$EVIDENCE/lean-model-build.log" 2>&1
LEAN_PATH="$EVIDENCE:$MODEL_PATH" bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" module "$PROOFS/Knowledge.lean" "$EVIDENCE/Knowledge.olean" > "$EVIDENCE/lean-knowledge.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-knowledge.log" 3
LEAN_PATH="$EVIDENCE:$MODEL_PATH" bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" module "$EVIDENCE/Generated.lean" > "$EVIDENCE/lean-generated.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-generated.log" 193
printf 'Endpoint ownership projection: Rust tests passed; 3 imported knowledge theorems; 193 generated claims. Evidence: %s\n' "$EVIDENCE"
