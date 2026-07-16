#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/lib/hygiene_common.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

# Size is primary. This gate only blocks structural hot-path regressions after
# check_final_form_measurements.sh has proven stack/SRAM/flash do not grow.

FAILED=0
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

assert_runtime_test_targets_are_unique() {
  python3 - "$0" <<'PY'
import re
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_text(encoding="utf-8")
start_marker = 'echo "== runtime performance operation-count tests =="'
end_marker = 'echo "== runtime cold compile-pressure test =="'
lines = source.splitlines()
try:
    start = next(idx for idx, line in enumerate(lines) if line.strip() == start_marker)
    end = next(
        idx for idx, line in enumerate(lines[start + 1 :], start + 1)
        if line.strip() == end_marker
    )
except StopIteration:
    raise SystemExit("runtime performance hygiene violation: operation-count test section missing")
body = "\n".join(lines[start + 1 : end])

targets = []
pending_test_arg = False
for raw_line in body.splitlines():
    line = raw_line.strip()
    if not line or line.startswith("#"):
        continue
    if pending_test_arg:
        targets.append(line.split()[0].rstrip("\\"))
        pending_test_arg = False
        continue
    match = re.match(r"--test(?:\s+(.+))?$", line.rstrip("\\").strip())
    if match is None:
        continue
    target = match.group(1)
    if target is None:
        pending_test_arg = True
    else:
        targets.append(target.split()[0].rstrip("\\"))

seen = set()
duplicates = []
for target in targets:
    if target in seen and target not in duplicates:
        duplicates.append(target)
    seen.add(target)
if duplicates:
    joined = ", ".join(duplicates)
    raise SystemExit(
        "runtime performance hygiene violation: cargo test target must be run once per script: "
        + joined
    )
PY
}

check_required_multiline \
  "fn next_set_from\\([^)]*\\)[[:space:]\n]*->[^{]+\\{[[:space:]\n\\S]*trailing_zeros\\(\\)" \
  "LaneSetView::next_set_from must advance over empty lane runs with bit operations" \
  "src/global/role_program/lane_set.rs"

check_required_multiline \
  "pub\\(crate\\) const fn route_scope_arm_lane_set_by_slot[[:space:]\n\\S]*route_scope_arm_lane_set_by_slot\\(" \
  "route-scope arm lane lookup must delegate to resident lane rows" \
  "src/global/role_program/image_impl/ref_access.rs"

check_required_multiline \
  "pub\\(crate\\) const fn route_scope_offer_lane_set_by_slot[[:space:]\n\\S]*route_scope_offer_lane_set_by_slot\\(" \
  "route-scope offer lane lookup must delegate to resident lane rows" \
  "src/global/role_program/image_impl/ref_access.rs"

check_absent_multiline \
  "pub\\(crate\\) fn phase_lane_set" \
  "resident phase lane-set accessor must not be detected as a runtime frontier surface" \
  "src/global/compiled/images/image/role_descriptor_ref.rs"

python3 - <<'PY'
from pathlib import Path

source = Path("src/global/role_program/image_impl/ref_access.rs").read_text(encoding="utf-8")

def section_between(start: str, end: str) -> str:
    try:
        tail = source.split(start, 1)[1]
    except IndexError:
        raise SystemExit(f"runtime performance hygiene violation: missing image section {start!r}")
    try:
        return tail.split(end, 1)[0]
    except IndexError:
        raise SystemExit(f"runtime performance hygiene violation: missing image section end {end!r}")

sections = {
    "route_scope_arm_lane_set_by_slot": section_between(
        "pub(crate) const fn route_scope_arm_lane_set_by_slot",
        "pub(crate) const fn route_scope_offer_lane_set_by_slot",
    ),
    "route_scope_offer_lane_set_by_slot": section_between(
        "pub(crate) const fn route_scope_offer_lane_set_by_slot",
        "pub(crate) const fn first_active_lane",
    ),
}

for name, section in sections.items():
    for forbidden in ["fill_role_atom_lanes_in_range", "view.len()", "while "]:
        if forbidden in section:
            raise SystemExit(
                "runtime performance hygiene violation: compiled image hot path "
                f"{name} must not rebuild lane sets by effect-list or full-view scans: {forbidden}"
            )
PY

check_absent_multiline \
  "route_scope_lane_words" \
  "endpoint arena must not contain route-scope lane-word caches" \
  "src/endpoint/kernel/decision_state.rs" \
  "src/endpoint/kernel/decision_state/route_arm_history.rs"

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "== runtime performance operation-count tests =="
assert_runtime_test_targets_are_unique

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
