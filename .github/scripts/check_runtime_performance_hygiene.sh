#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv"

run_runtime_test() {
  local label="$1"
  shift 1
  local output
  local observed
  local -a cargo_env
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-runtime-performance.XXXXXX")"
  cargo_env=()
  if [[ -n "${HIBANA_RUNTIME_TEST_TARGET_DIR:-}" ]]; then
    cargo_env=(env CARGO_TARGET_DIR="${HIBANA_RUNTIME_TEST_TARGET_DIR}")
  fi
  set +e
  HIBANA_COMPILE_PRESSURE_LABEL="${label}" \
    HIBANA_COMPILE_PRESSURE_BUDGETS="${COMPILE_PRESSURE_BUDGETS}" \
    run_with_compile_pressure_guard \
      "runtime ${label}" \
      bash -c 'exec "$@" 2>&1' bash "${cargo_env[@]}" cargo +"${TOOLCHAIN}" test "$@" \
    | tee "${output}"
  local status="${PIPESTATUS[0]}"
  set -e
  if [[ "${status}" -ne 0 ]]; then
    rm -f "${output}"
    exit 1
  fi
  observed="$(grep -E "^compile pressure observed: runtime ${label} " "${output}" | tail -n 1)"
  if [[ ! "${observed}" =~ elapsed=([0-9]+)s[[:space:]]seconds_budget=([0-9]+)s[[:space:]]max_rss=([0-9]+)MiB[[:space:]]rss_budget=([0-9]+)MiB ]]; then
    rm -f "${output}"
    echo "runtime performance hygiene violation: missing aggregate compile pressure observation for ${label}" >&2
    exit 1
  fi
  echo "runtime compile pressure label=${label} elapsed=${BASH_REMATCH[1]}s seconds_budget=${BASH_REMATCH[2]}s max_rss=${BASH_REMATCH[3]}MiB rss_budget=${BASH_REMATCH[4]}MiB"
  if ! grep -Eq "running [1-9][0-9]* tests?" "${output}"; then
    rm -f "${output}"
    echo "runtime performance hygiene violation: cargo test filter matched no tests: $*" >&2
    exit 1
  fi
  rm -f "${output}"
}

echo "== runtime performance operation-count tests =="

run_runtime_test \
  "offer_branch_recv_evidence" \
  -p hibana \
  --test offer_branch_recv_evidence

run_runtime_test \
  "parallel_route_nesting" \
  -p hibana \
  --test parallel_route_nesting

run_runtime_test \
  "parallel_route_alternating" \
  -p hibana \
  --test parallel_route_alternating

run_runtime_test \
  "huge_choreography_runtime" \
  -p hibana \
  --test huge_choreography_runtime

echo "== runtime cold compile-pressure test =="
cold_target_dir="$(mktemp -d "${TMPDIR:-/tmp}/hibana-runtime-cold-target.XXXXXX")"
cleanup_cold_target_dir() {
  rm -rf "${cold_target_dir}"
}
trap cleanup_cold_target_dir EXIT
HIBANA_RUNTIME_TEST_TARGET_DIR="${cold_target_dir}" run_runtime_test \
  "cold_parallel_route_nesting" \
  -p hibana \
  --test parallel_route_nesting
trap - EXIT
cleanup_cold_target_dir

echo "runtime performance hygiene check passed"
