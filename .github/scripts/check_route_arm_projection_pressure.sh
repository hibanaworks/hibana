#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
export CARGO_BUILD_JOBS=1
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"
TARGET="thumbv6m-none-eabi"
rustup target add --toolchain "${TOOLCHAIN}" "${TARGET}" >/dev/null

MATRIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-route-arm-pressure-XXXXXX")"
TARGET_DIR="${MATRIX_DIR}/target"
cleanup() {
  rm -rf "${MATRIX_DIR}"
}
trap cleanup EXIT

generate_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/route_arm_${count}"
  mkdir -p "${crate_dir}/src"

  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "hibana-route-arm-heavy-${count}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_route_arm_heavy_${count}"

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }
EOF

  bash "${ROOT_DIR}/.github/scripts/lib/pressure_source_tool.sh" route unused "${count}" "${crate_dir}/src/lib.rs"
}

declare -A COMPILE_SECONDS=()
declare -A COMPILE_RSS_MIB=()

build_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/route_arm_${count}"
  local crate_name="hibana_route_arm_heavy_${count}"
  local output
  local observed
  local status

  generate_case "${count}"
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-route-arm-pressure.XXXXXX")"
  set +e
  HIBANA_COMPILE_PRESSURE_LABEL="route_arm_heavy_${count}" \
    HIBANA_COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv" \
    HIBANA_COMPILE_PRESSURE_CRATE_NAME="${crate_name}" \
    HIBANA_COMPILE_PRESSURE_POLL_SECONDS=0.1 \
    run_with_compile_pressure_guard \
      "route-arm-heavy ${count}" \
      env \
        PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
        RUSTC="${TOOLCHAIN_RUSTC}" \
        CARGO_TERM_COLOR=never \
        CARGO_TERM_PROGRESS_WHEN=never \
        TERM=dumb \
        CARGO_TARGET_DIR="${TARGET_DIR}" \
        "${TOOLCHAIN_CARGO}" build \
          --manifest-path "${crate_dir}/Cargo.toml" \
          --no-default-features \
          --target "${TARGET}" \
          --release \
          --lib \
      2>&1 | tee "${output}"
  status="${PIPESTATUS[0]}"
  set -e
  if [[ "${status}" -ne 0 ]]; then
    rm -f "${output}"
    exit "${status}"
  fi
  observed="$(bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" observation "${output}" "compile pressure observed: route-arm-heavy ${count} ")"
  rm -f "${output}"
  read -r "COMPILE_SECONDS[${count}]" "COMPILE_RSS_MIB[${count}]" <<< "${observed}"
}

for count in 1 64 256; do
  build_case "${count}"
done

# Each independently compiled case is already bounded by its named snapshot-derived
# time and RSS budgets. Cross-case comparisons of sampled peaks are not stable gates.

echo "route-arm projection pressure passed target=${TARGET} arm-events=256 elapsed=${COMPILE_SECONDS[256]}s rss=${COMPILE_RSS_MIB[256]}MiB"
