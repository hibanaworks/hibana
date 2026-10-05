#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if [[ "${HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD:-1}" != "0" \
  && "${HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE:-0}" != "1" ]]; then
  source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
  HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE=1 \
    HIBANA_COMPILE_PRESSURE_LABEL=final_form_gate \
    HIBANA_COMPILE_PRESSURE_CRATE_NAME=hibana \
    run_with_compile_pressure_guard \
      "final-form gate" \
      env HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE=1 bash "$0" "$@"
  exit "$?"
fi

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg

bash ./.github/scripts/check_rust_1_95_stable.sh
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
CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  cargo +"${TOOLCHAIN}" test --locked -p hibana --release \
    --test rolled_publication_exit -- --test-threads=1
bash ./.github/scripts/check_miri.sh
bash ./.github/scripts/check_lean_proofs.sh
bash proofs/controller-offer/check.sh
bash proofs/live-descendant-preview/check.sh
ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0 \
  PYTHONDONTWRITEBYTECODE=1 python3 -B proofs/elastic-roll-colors/check_all.py --lean lean
ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0 \
  PYTHONDONTWRITEBYTECODE=1 python3 -B proofs/wire-frame-refinement/check_all.py --lean lean
bash ./.github/scripts/check_unix_carrier_proof.sh
bash ./.github/scripts/check_final_form_measurements.sh
bash ./.github/scripts/check_runtime_performance_hygiene.sh
bash ./.github/scripts/check_subsystem_budget_gates.sh
bash ./.github/scripts/check_package_artifact.sh
