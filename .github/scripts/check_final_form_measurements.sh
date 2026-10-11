#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

RUSTUP=(rustup run "${TOOLCHAIN}")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"

rustup component add llvm-tools-preview --toolchain "${TOOLCHAIN}" >/dev/null

SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc --print host-tuple)"
RUST_BIN_DIR="${SYSROOT}/lib/rustlib/${HOST}/bin"

if [[ -x "${RUST_BIN_DIR}/llvm-size" ]]; then
  LLVM_SIZE="${RUST_BIN_DIR}/llvm-size"
elif command -v llvm-size >/dev/null 2>&1; then
  LLVM_SIZE="$(command -v llvm-size)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-size ]]; then
  LLVM_SIZE="/opt/homebrew/opt/llvm/bin/llvm-size"
else
  echo "final-form measurements require llvm-size" >&2
  exit 1
fi

if [[ -x "${RUST_BIN_DIR}/llvm-nm" ]]; then
  LLVM_NM="${RUST_BIN_DIR}/llvm-nm"
elif command -v llvm-nm >/dev/null 2>&1; then
  LLVM_NM="$(command -v llvm-nm)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-nm ]]; then
  LLVM_NM="/opt/homebrew/opt/llvm/bin/llvm-nm"
else
  echo "final-form measurements require llvm-nm" >&2
  exit 1
fi

run_final_form_test() {
  local output
  if ! output="$(cargo +"${TOOLCHAIN}" test "$@" 2>&1)"; then
    printf '%s\n' "${output}"
    exit 1
  fi
  printf '%s\n' "${output}"
  if ! bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" nonempty-test <<<"${output}"; then
    echo "final-form measurement violation: cargo test filter matched no tests: $*" >&2
    exit 1
  fi
}

MEASURE_TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
MEASURE_DIR="${MEASURE_TARGET_DIR}/final_form_measurements"
SNAPSHOT_FILE="${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json"
rm -rf "${MEASURE_DIR}"
mkdir -p "${MEASURE_DIR}/src"

bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" host-project "${ROOT_DIR}" "${MEASURE_DIR}"

PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
RUSTC="${TOOLCHAIN_RUSTC}" \
CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
TERM=dumb \
  "${TOOLCHAIN_CARGO}" build \
    --manifest-path "${MEASURE_DIR}/Cargo.toml" \
    --release \
    --target-dir "${MEASURE_DIR}/target" \
    >/dev/null

BIN="${MEASURE_DIR}/target/release/hibana-final-form-measure"
if [[ ! -f "${BIN}" ]]; then
  echo "final-form measurement binary missing: ${BIN}" >&2
  exit 1
fi

echo "== final-form binary sections =="
"${LLVM_SIZE}" --format=sysv "${BIN}" \
  | bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" llvm-sections host

echo "== final-form thumb/no-std binary sections =="
rustup target add --toolchain "${TOOLCHAIN}" thumbv6m-none-eabi >/dev/null
CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
TERM=dumb \
  cargo +"${TOOLCHAIN}" build \
    -p hibana \
    --no-default-features \
    --target thumbv6m-none-eabi \
    --release \
    --lib \
    >/dev/null
THUMB_RLIB="${MEASURE_TARGET_DIR}/thumbv6m-none-eabi/release/libhibana.rlib"
if [[ ! -f "${THUMB_RLIB}" ]]; then
  echo "final-form thumb measurement artifact missing: ${THUMB_RLIB}" >&2
  exit 1
fi
TOOLCHAIN="${TOOLCHAIN}" bash "${ROOT_DIR}/.github/scripts/check_thumbv6m_frame_header_codegen.sh"
THUMB_SECTION_OUTPUT="$("${LLVM_SIZE}" --format=sysv "${THUMB_RLIB}" \
  | bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" llvm-sections thumb)"
printf '%s\n' "${THUMB_SECTION_OUTPUT}"
if [[ "${HIBANA_OMIT_FIXED_SNAPSHOT_CHECK:-0}" != "1" ]]; then
THUMB_SECTION_OUTPUT="${THUMB_SECTION_OUTPUT}" SNAPSHOT_FILE="${SNAPSHOT_FILE}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" thumb
else
  echo "fixed snapshot thumb budget check omitted by explicit override; worktree size snapshot still runs"
fi

echo "== final-form future/layout sizes =="
run_final_form_test -p hibana endpoint_surface_size_gates_hold --lib
run_final_form_test -p hibana recv_future_state_caches_completion --lib
run_final_form_test -p hibana message_type_variation_does_not_change_future_layout --lib
run_final_form_test -p hibana send_future_and_runtime_descriptor_size_gates_hold --lib
FUTURE_LAYOUT_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test -p hibana final_form_future_layout_measurement_report --lib -- --nocapture
)"
printf '%s\n' "${FUTURE_LAYOUT_OUTPUT}"
FUTURE_LAYOUT_OUTPUT="${FUTURE_LAYOUT_OUTPUT}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" layout

echo "== final-form projected protocol matrix =="
PROTOCOL_MATRIX_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test \
    -p hibana \
    global::role_program::tests::protocol_matrix::projected_protocol_matrix_reports_compact_resident_images \
    --lib \
    -- \
    --exact \
    --nocapture
)"
printf '%s\n' "${PROTOCOL_MATRIX_OUTPUT}"
PROTOCOL_MATRIX_OUTPUT="${PROTOCOL_MATRIX_OUTPUT}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" matrix

echo "== final-form protocol artifact flash matrix =="
PROTOCOL_ARTIFACT_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/hibana-protocol-artifacts-XXXXXX")"
cleanup_protocol_artifacts() {
  rm -rf "${PROTOCOL_ARTIFACT_ROOT}"
}
trap cleanup_protocol_artifacts EXIT

write_protocol_artifact_source() {
  bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" artifact-project "${ROOT_DIR}" "$1" "$2"
}

protocol_matrix_metrics_for() {
  PROTOCOL_MATRIX_OUTPUT="${PROTOCOL_MATRIX_OUTPUT}" PROTOCOL_NAME="$1" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" protocol-metrics
}

protocol_artifact_role_bucket_count() {
  bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" role-count "$1"
}

protocol_artifact_map_metrics() {
  MAP_PATH="$1" BIN_PATH="$2" LLVM_NM="${LLVM_NM}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" map
}

protocol_artifact_aeabi_metrics() {
  BIN_PATH="$1" LLVM_NM="${LLVM_NM}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" aeabi
}

build_protocol_artifact() {
  local protocol_name="$1"
  local package_name="hibana-protocol-${protocol_name//_/-}"
  local crate_dir="${PROTOCOL_ARTIFACT_ROOT}/${protocol_name}"
  local map="${crate_dir}/firmware.map"
  write_protocol_artifact_source "${crate_dir}" "${protocol_name}"
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  RUSTC="${TOOLCHAIN_RUSTC}" \
  RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-e -C link-arg=_start -C link-arg=-Map=${map}" \
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${crate_dir}/Cargo.toml" \
      --target thumbv6m-none-eabi \
      --release \
      --target-dir "${crate_dir}/target" \
      >/dev/null
  local bin="${crate_dir}/target/thumbv6m-none-eabi/release/${package_name}"
  if [[ ! -f "${bin}" ]]; then
    echo "protocol artifact missing: ${bin}" >&2
    exit 1
  fi
  local section_metrics
  section_metrics="$("${LLVM_SIZE}" --format=sysv "${bin}" \
    | bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" llvm-sections artifact)"
  printf 'protocol-artifact name=%s %s %s\n' \
    "${protocol_name}" \
    "$(protocol_matrix_metrics_for "${protocol_name}")" \
    "selected_program_bucket_count=1 selected_role_bucket_count=$(protocol_artifact_role_bucket_count "${protocol_name}") ${section_metrics} $(protocol_artifact_map_metrics "${map}" "${bin}") $(protocol_artifact_aeabi_metrics "${bin}")"
}

PROTOCOL_ARTIFACT_OUTPUT="$(
  for protocol_name in \
    minimal_send_recv \
    nested_par_join \
    route_with_unselected_nested_par \
    triple_nested_route \
    passive_nested_route_observer \
    alternating_par_route \
    huge_legal_choreography
  do
    build_protocol_artifact "${protocol_name}"
  done
)"
printf '%s\n' "${PROTOCOL_ARTIFACT_OUTPUT}"
PROTOCOL_ARTIFACT_OUTPUT="${PROTOCOL_ARTIFACT_OUTPUT}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" artifacts

echo "== final-form runtime stack high-water =="
STACK_HIGH_WATER_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test \
    -p hibana \
    large_choreography_runtime_peak_metrics \
    --lib \
    --release \
    -- \
    --ignored \
    --nocapture \
    --test-threads=1
)"
printf '%s\n' "${STACK_HIGH_WATER_OUTPUT}"
if [[ "${HIBANA_OMIT_FIXED_SNAPSHOT_CHECK:-0}" != "1" ]]; then
STACK_HIGH_WATER_OUTPUT="${STACK_HIGH_WATER_OUTPUT}" THUMB_SECTION_OUTPUT="${THUMB_SECTION_OUTPUT}" SNAPSHOT_FILE="${SNAPSHOT_FILE}" bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" runtime
else
  echo "fixed snapshot runtime budget check omitted by explicit override; worktree size snapshot still runs"
fi

MEASUREMENT_HOST="${HOST}" \
THUMB_SECTION_OUTPUT="${THUMB_SECTION_OUTPUT}" \
PROTOCOL_ARTIFACT_OUTPUT="${PROTOCOL_ARTIFACT_OUTPUT}" \
STACK_HIGH_WATER_OUTPUT="${STACK_HIGH_WATER_OUTPUT}" \
bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" live

if [[ "${HIBANA_OMIT_WORKTREE_SIZE_SNAPSHOT:-0}" != "1" ]]; then
  echo "== final-form worktree size snapshot =="
  HIBANA_OMIT_FIXED_SNAPSHOT_CHECK=1 \
    bash "${ROOT_DIR}/.github/scripts/check_size_snapshot_regression.sh"
fi

echo "== final-form message-heavy matrix =="
bash "${ROOT_DIR}/.github/scripts/check_message_heavy_matrix.sh"

echo "== final-form route-arm projection pressure =="
bash "${ROOT_DIR}/.github/scripts/check_route_arm_projection_pressure.sh"

echo "== final-form causal-handoff pressure =="
bash "${ROOT_DIR}/.github/scripts/check_causal_handoff_pressure.sh"

echo "final-form measurement check passed"
