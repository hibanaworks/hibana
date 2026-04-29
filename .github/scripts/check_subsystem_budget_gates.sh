#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/Cargo.toml"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

# Compiled-role and resident atlas budgets.
cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  control::cluster::core::tests::pico2_resident_component_sizes \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  control::cluster::core::tests::huge_shape_matrix_resident_bytes_stay_measured_and_local \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  global::compiled::role::tests::huge_route_heavy_shape_keeps_resident_bounds_local \
  -- \
  --exact \
  --nocapture

# Send/policy hot-path ownership.
cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  endpoint::kernel::core::route_policy_tests::route_policy_action_mapping_is_explicit \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --lib \
  --features std \
  endpoint::kernel::core::route_policy_tests::route_policy_scope_mismatch_blocks_resolver_delegation \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  --features std \
  offer_kernel_stays_three_stage_and_fail_closed \
  -- \
  --exact \
  --nocapture
