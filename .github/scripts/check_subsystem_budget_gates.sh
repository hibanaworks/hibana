#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/Cargo.toml"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

run_subsystem_budget_test() {
  local output
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-subsystem-budget.XXXXXX")"
  if ! cargo +"${TOOLCHAIN}" test "$@" >"${output}" 2>&1; then
    cat "${output}"
    rm -f "${output}"
    exit 1
  fi
  cat "${output}"
  if ! bash "${ROOT_DIR}/.github/scripts/lib/test_result_tool.sh" check "${output}" "subsystem budget" 0; then
    rm -f "${output}"
    exit 1
  fi
  rm -f "${output}"
}

# Compiled-role and resident atlas budgets.
run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  endpoint::tests::endpoint_surface_size_gates_hold \
  -- \
  --exact \
  --nocapture

run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  endpoint::tests::send_future_and_runtime_descriptor_size_gates_hold \
  -- \
  --exact \
  --nocapture

run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  global::typestate::facts::tests::inbound_frame_key_is_exact_three_byte_identity \
  -- \
  --exact \
  --nocapture

run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  global::role_program::tests::protocol_matrix::projected_protocol_matrix_reports_compact_resident_images \
  -- \
  --exact \
  --nocapture
