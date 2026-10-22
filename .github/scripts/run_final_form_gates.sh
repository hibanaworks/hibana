#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
export TOOLCHAIN="${TOOLCHAIN:-stable}"
# Retain source locations without full type/value DWARF for the projection
# test matrix. Debug assertions and overflow checks remain enabled.
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg

cargo +"${TOOLCHAIN}" test --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin resource-guard
bash ./.github/scripts/check_rust_stable.sh
bash ./.github/scripts/check_no_std_build.sh
cargo +"${TOOLCHAIN}" clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo +"${TOOLCHAIN}" doc -p hibana --no-deps --document-private-items
PING_PONG_OUTPUT="$(
  (
    hibana_disable_repo_tests_cfg
    cargo +"${TOOLCHAIN}" run --quiet --example ping_pong 2>&1
  )
)"
if [[ "${PING_PONG_OUTPUT}" != "ping=7, pong=8" ]]; then
  echo "ping_pong example output mismatch: ${PING_PONG_OUTPUT}" >&2
  exit 1
fi
DELEGATION_OUTPUT="$(
  (
    hibana_disable_repo_tests_cfg
    cargo +"${TOOLCHAIN}" run --quiet --locked --example endpoint_delegation 2>&1
  )
)"
if [[ "${DELEGATION_OUTPUT}" != "worker=8, owner=42, audit=7" ]]; then
  echo "endpoint_delegation example output mismatch: ${DELEGATION_OUTPUT}" >&2
  exit 1
fi
(
  hibana_disable_repo_tests_cfg
  cargo +"${TOOLCHAIN}" test --locked --example endpoint_delegation
)
CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  cargo +"${TOOLCHAIN}" test --locked -p hibana --release \
    --test rolled_publication_exit -- --test-threads=1
bash ./.github/scripts/check_lean_proofs.sh
bash ./.github/scripts/check_miri.sh
exhaustive_output="$(mktemp "${TMPDIR:-/tmp}/hibana-exhaustive.XXXXXX")"
trap 'rm -f "$exhaustive_output"' EXIT
if CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true \
  cargo +"${TOOLCHAIN}" test --locked -p hibana --lib --release \
    global::const_dsl::receive_lane_causality::actors::exhaustive \
    -- --ignored --nocapture --test-threads=1 >"$exhaustive_output" 2>&1; then
  exhaustive_status=0
else
  exhaustive_status=$?
fi
cat "$exhaustive_output"
bash .github/scripts/lib/test_result_tool.sh check \
  "$exhaustive_output" exhaustive-route-correlation "$exhaustive_status"
rm -f "$exhaustive_output"
trap - EXIT
bash proofs/checks/runtime/controller-offer/check.sh
bash proofs/checks/runtime/live-descendant-preview/check.sh
bash proofs/checks/runtime/nested-resolver-reentry/check.sh
bash proofs/checks/runtime/ordinary-offer-ingress/check.sh
bash proofs/checks/runtime/offer-prefix-dependency/check.sh
bash proofs/checks/projection/endpoint-delegation/check.sh
ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0 \
  bash .github/scripts/check_projection_algorithms.sh --lean lean
bash ./.github/scripts/check_unix_carrier_proof.sh
bash ./.github/scripts/check_final_form_measurements.sh
bash ./.github/scripts/check_runtime_performance_hygiene.sh
bash ./.github/scripts/check_subsystem_budget_gates.sh
bash ./.github/scripts/check_package_artifact.sh
