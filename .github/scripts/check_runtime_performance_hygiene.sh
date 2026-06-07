#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

# Size is primary. This gate only blocks structural hot-path regressions after
# check_final_form_measurements.sh has proven stack/SRAM/flash do not grow.

require_source() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -q --multiline "${pattern}" "${file}"; then
    echo "runtime performance hygiene violation: ${message}" >&2
    exit 1
  fi
}

reject_source() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if rg -q --multiline "${pattern}" "${file}"; then
    echo "runtime performance hygiene violation: ${message}" >&2
    exit 1
  fi
}

require_source \
  "src/global/role_program/lane_set.rs" \
  "fn next_set_from\\([^)]*\\)[[:space:]\n]*->[^{]+\\{[[:space:]\n\\S]*trailing_zeros\\(\\)" \
  "LaneSetView::next_set_from must skip empty lane runs with bit operations"

require_source \
  "src/global/compiled/images/image/role_descriptor_ref/route_scope.rs" \
  "pub\\(crate\\) fn route_scope_arm_lane_set_by_slot[[:space:]\n\\S]*route_scope_arm_lane_set_by_slot\\(" \
  "route-scope arm lane lookup must delegate to resident lane rows"

require_source \
  "src/global/compiled/images/image/role_descriptor_ref/route_scope.rs" \
  "pub\\(crate\\) fn route_scope_offer_lane_set_by_slot[[:space:]\n\\S]*route_scope_offer_lane_set_by_slot\\(" \
  "route-scope offer lane lookup must delegate to resident lane rows"

reject_source \
  "src/global/compiled/images/image/role_descriptor_ref.rs" \
  "pub\\(crate\\) fn phase_lane_set" \
  "resident phase lane-set accessor must not be reintroduced as a runtime frontier surface"

python3 - <<'PY'
from pathlib import Path

source = (
    Path("src/global/compiled/images/image/role_descriptor_ref.rs").read_text(encoding="utf-8")
    + "\n"
    + Path("src/global/compiled/images/image/role_descriptor_ref/route_scope.rs").read_text(encoding="utf-8")
)

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
        "pub(crate) fn route_scope_arm_lane_set_by_slot",
        "pub(crate) fn route_scope_offer_lane_set_by_slot",
    ),
    "route_scope_offer_lane_set_by_slot": section_between(
        "pub(crate) fn route_scope_offer_lane_set_by_slot",
        "pub(crate) fn route_scope_offer_entry_by_slot",
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

reject_source \
  "src/endpoint/kernel/decision_state.rs" \
  "route_scope_lane_words" \
  "endpoint arena must not reintroduce route-scope lane-word caches"

echo "== runtime performance operation-count tests =="
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test offer_decode_receive_evidence \
  offer_requires_framed_receive_evidence_for_branch_demux \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test offer_decode_receive_evidence \
  offer_decode_transport_consumes_frame_once \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test offer_decode_receive_evidence \
  forgotten_route_branch_leaves_endpoint_fail_closed \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test offer_decode_receive_evidence \
  forgotten_decode_future_leaves_endpoint_fail_closed \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test parallel_route_nesting \
  route_inside_parallel_lane_cannot_release_join_before_sibling_lane \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test parallel_route_nesting \
  alternating_route_parallel_join_uses_only_selected_arms \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test parallel_route_nesting \
  unselected_route_arm_parallel_events_are_dead_and_not_join_obligations \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test parallel_route_nesting \
  unselected_route_arm_parallel_events_do_not_block_parallel_join \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --test parallel_route_nesting \
  outer_left_selection_kills_nested_right_route_and_parallel_body \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  global::role_program::tests::lane_set_view_iterates_set_bits_without_empty_lane_scan \
  --lib \
  --features std

echo "runtime performance hygiene check passed"
