#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT}/.github/scripts/ensure_rust_toolchain.sh"

run_shape() {
  local name="$1"
  local features="$2"
  local target_dir="$ROOT/target/pico_size_matrix/$name"

  echo "== pico size matrix: $name =="
  if [[ -n "$features" ]]; then
    HIBANA_PICO_TOOLCHAIN="$TOOLCHAIN" \
    HIBANA_PICO_TARGET_DIR="$target_dir" \
    HIBANA_PICO_FEATURES="$features" \
      bash "$ROOT/.github/scripts/check_pico_smoke.sh"
  else
    HIBANA_PICO_TOOLCHAIN="$TOOLCHAIN" \
    HIBANA_PICO_TARGET_DIR="$target_dir" \
      bash "$ROOT/.github/scripts/check_pico_smoke.sh"
  fi
}

run_shape route_heavy ""
run_shape linear_heavy "linear-heavy"
run_shape fanout_heavy "fanout-heavy"

echo "== resident size matrix =="
cargo +"${TOOLCHAIN}" test -p hibana huge_shape_matrix_resident_bytes_stay_measured_and_local --lib --features std -- --nocapture
