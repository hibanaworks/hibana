#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-stable}"
export CARGO_BUILD_JOBS=1
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"
TARGET="thumbv6m-none-eabi"
rustup target add --toolchain "${TOOLCHAIN}" "${TARGET}" >/dev/null

MATRIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-causal-handoff-pressure-XXXXXX")"
TARGET_DIR="${MATRIX_DIR}/target"
cleanup() {
  rm -rf "${MATRIX_DIR}"
}
trap cleanup EXIT

generate_case() {
  local shape="$1"
  local count="$2"
  local crate_dir="${MATRIX_DIR}/causal_handoff_${shape}_${count}"
  mkdir -p "${crate_dir}/src"

  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "hibana-causal-handoff-${shape}-${count}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_causal_handoff_${shape}_${count}"

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }
EOF

  local generator=causal
  if [[ "$shape" == mixed || "$shape" == hierarchical ]]; then
    generator=orchestration
  fi
  bash "${ROOT_DIR}/.github/scripts/lib/pressure_source_tool.sh" "$generator" "${shape}" "${count}" "${crate_dir}/src/lib.rs"
}

declare -A COMPILE_SECONDS=()
declare -A COMPILE_RSS_MIB=()

build_case() {
  local shape="$1"
  local count="$2"
  local crate_dir="${MATRIX_DIR}/causal_handoff_${shape}_${count}"
  local crate_name="hibana_causal_handoff_${shape}_${count}"
  local pressure_label="causal_handoff_${shape}_${count}"
  local output
  local observed
  local status

  generate_case "${shape}" "${count}"
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-causal-handoff-pressure.XXXXXX")"
  set +e
  if [[ "${shape}" == "linear" ]]; then
    pressure_label="causal_handoff_${count}"
  elif [[ "${shape}" == "capacity" ]]; then
    pressure_label="global_capacity_${count}"
  elif [[ "$shape" == mixed || "$shape" == hierarchical ]]; then
    pressure_label="orchestration_${shape}_${count}"
  fi
  HIBANA_COMPILE_PRESSURE_LABEL="${pressure_label}" \
    HIBANA_COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv" \
    HIBANA_COMPILE_PRESSURE_CRATE_NAME="${crate_name}" \
    HIBANA_COMPILE_PRESSURE_POLL_SECONDS=0.1 \
    run_with_compile_pressure_guard \
      "causal-handoff ${shape} ${count}" \
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
  observed="$(bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" observation "${output}" "compile pressure observed: causal-handoff ${shape} ${count} ")"
  rm -f "${output}"
  read -r "COMPILE_SECONDS[${shape}_${count}]" "COMPILE_RSS_MIB[${shape}_${count}]" <<< "${observed}"
}

for count in 4 64 256; do
  build_case linear "${count}"
done
for shape in route roll; do
  for count in 4 32 64; do
    build_case "${shape}" "${count}"
  done
done
for count in 64 128 256; do
  build_case delegation "${count}"
done
build_case alternating 1024
build_case alternating 4096
build_case receive_cycles 512
build_case capacity 7281
build_case mixed 100
build_case hierarchical 100

# Each independently compiled case is already bounded by its named snapshot-derived
# time and RSS budgets. Cross-case comparisons of sampled peaks are not stable gates.

echo "causal-handoff pressure passed target=${TARGET} linear-events=256 route-arm-events=64 roll-events=64 receive-cycle-regions=512 receive-cycle-events=1537 receive-cycle-roles=8 left-associated-transfers=256 alternating-events=4096 global-capacity-events=7281 orchestration-roles=100 orchestration-events=4000 linear-elapsed=${COMPILE_SECONDS[linear_256]}s route-elapsed=${COMPILE_SECONDS[route_64]}s roll-elapsed=${COMPILE_SECONDS[roll_64]}s delegation-elapsed=${COMPILE_SECONDS[delegation_256]}s alternating-elapsed=${COMPILE_SECONDS[alternating_4096]}s capacity-elapsed=${COMPILE_SECONDS[capacity_7281]}s linear-rss=${COMPILE_RSS_MIB[linear_256]}MiB route-rss=${COMPILE_RSS_MIB[route_64]}MiB roll-rss=${COMPILE_RSS_MIB[roll_64]}MiB delegation-rss=${COMPILE_RSS_MIB[delegation_256]}MiB alternating-rss=${COMPILE_RSS_MIB[alternating_4096]}MiB capacity-rss=${COMPILE_RSS_MIB[capacity_7281]}MiB receive-cycles-elapsed=${COMPILE_SECONDS[receive_cycles_512]}s receive-cycles-rss=${COMPILE_RSS_MIB[receive_cycles_512]}MiB"
