#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/Cargo.toml"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

# Compiled-role and resident atlas budgets.
cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  control::cluster::core::tests::resident_shape::pico2_resident_component_sizes \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  control::cluster::core::tests::resident_shape::huge_shape_matrix_resident_bytes_stay_measured_and_local \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  global::role_program::tests::tests::projected_protocol_matrix_reports_compact_resident_images \
  -- \
  --exact \
  --nocapture

# Send/policy hot-path ownership.
cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  endpoint::kernel::core::decision_policy_tests::decision_policy_input_arg0_defaults_to_zero \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  --features std \
  dynamic_policy_surface_uses_one_decision_resolver \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  --features std \
  transport_context_keeps_resolver_input_only \
  -- \
  --exact \
  --nocapture
