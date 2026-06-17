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
  if ! output="$(cargo +"${TOOLCHAIN}" test "$@" 2>&1)"; then
    printf '%s\n' "${output}"
    exit 1
  fi
  printf '%s\n' "${output}"
  if ! grep -Eq "running [1-9][0-9]* tests?" <<<"${output}"; then
    echo "subsystem budget gate violation: cargo test filter matched no tests: $*" >&2
    exit 1
  fi
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
  endpoint::kernel::evidence::tests::scope_frame_label_meta_size_budget \
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

# Send/resolver hot-path ownership.
run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  core_resolver_audit_has_no_in_crate_resolver_owner \
  -- \
  --exact \
  --nocapture

run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  dynamic_resolver_surface_uses_one_decision_resolver \
  -- \
  --exact \
  --nocapture

run_subsystem_budget_test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  transport_context_owner_stays_forbidden \
  -- \
  --exact \
  --nocapture
