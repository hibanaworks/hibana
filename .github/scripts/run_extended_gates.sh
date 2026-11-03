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

CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  cargo +"${TOOLCHAIN}" test --locked -p hibana --release \
    --test rolled_publication_exit -- --test-threads=1
bash ./.github/scripts/check_control_actor_sensitivity.sh
bash ./.github/scripts/check_scope_reentry_sensitivity.sh
bash ./.github/scripts/check_send_poll_sensitivity.sh
bash ./.github/scripts/check_pending_acceptance_sensitivity.sh
bash ./.github/scripts/check_lean_proofs.sh --generated
bash proofs/checks/projection/endpoint-delegation/check.sh
bash ./.github/scripts/check_miri.sh --extended
bash ./.github/scripts/check_unix_carrier_proof.sh
bash ./.github/scripts/check_final_form_measurements.sh
bash ./.github/scripts/check_runtime_performance_hygiene.sh
bash ./.github/scripts/check_package_artifact.sh

bash .github/scripts/check_nested_roll_admission.sh
bash proofs/checks/runtime/rolled-route-ownership/check.sh
miri="$(cat .github/miri-toolchain)"
for case in $(seq 0 15); do
  out="target/miri-parallel-reentry/$case"
  mkdir -p "$out"
  MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-env-set=HIBANA_PARALLEL_CREDIT_CASE=$case" \
    timeout --kill-after=10s 480s cargo +"$miri" miri test --locked --test nested_parallel_reentry \
      nested_parallel_delegation_preserves_a_child_when_its_parent_restarts_late \
      -- --exact --nocapture > "$out/run.log" 2>&1
  reverse=false
  if (( case % 2 )); then reverse=true; fi
  grep -F "parallel-credit complete: right_visit=$((case / 2)) reverse=$reverse" "$out/run.log"
  test "$(grep -c '^parallel-credit complete:' "$out/run.log")" = 1
done
