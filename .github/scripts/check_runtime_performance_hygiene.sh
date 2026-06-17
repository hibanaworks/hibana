#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/lib/hygiene_common.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

# Size is primary. This gate only blocks structural hot-path regressions after
# check_final_form_measurements.sh has proven stack/SRAM/flash do not grow.

FAILED=0

run_runtime_test() {
  local output
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-runtime-performance.XXXXXX")"
  if ! cargo +"${TOOLCHAIN}" test "$@" 2>&1 | tee "${output}"; then
    rm -f "${output}"
    exit 1
  fi
  if ! grep -Eq "running [1-9][0-9]* tests?" "${output}"; then
    rm -f "${output}"
    echo "runtime performance hygiene violation: cargo test filter matched no tests: $*" >&2
    exit 1
  fi
  rm -f "${output}"
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
  "src/endpoint/kernel/decision_state.rs"

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "== runtime performance operation-count tests =="
run_runtime_test \
	  -p hibana \
	  --test offer_decode_receive_evidence \
	  offer_requires_framed_receive_evidence_for_branch_demux \

	run_runtime_test \
	  -p hibana \
	  --test offer_decode_receive_evidence \
	  offer_decode_transport_consumes_frame_once \

	run_runtime_test \
	  -p hibana \
	  --test offer_decode_receive_evidence \
	  forgotten_route_branch_leaves_endpoint_fail_closed \

	run_runtime_test \
	  -p hibana \
	  --test offer_decode_receive_evidence \
	  forgotten_route_recv_future_leaves_endpoint_fail_closed \

	run_runtime_test \
	  -p hibana \
	  --test parallel_route_nesting \
	  route_inside_parallel_lane_cannot_release_join_before_sibling_lane \

	run_runtime_test \
	  -p hibana \
	  --test parallel_route_alternating \
	  alternating_route_parallel_join_uses_only_selected_arms \

	run_runtime_test \
	  -p hibana \
	  --test parallel_route_nesting \
	  unselected_route_arm_parallel_events_are_dead_and_not_join_obligations \

	run_runtime_test \
	  -p hibana \
	  --test parallel_route_nesting \
	  unselected_route_arm_parallel_events_do_not_block_parallel_join \

	run_runtime_test \
	  -p hibana \
	  --test parallel_route_nesting \
	  outer_left_selection_kills_nested_right_route_and_parallel_body \

	run_runtime_test \
	  -p hibana \
	  global::role_program::tests::lane_set_view_iterates_set_bits_without_empty_lane_scan \
	  --lib \


echo "runtime performance hygiene check passed"
