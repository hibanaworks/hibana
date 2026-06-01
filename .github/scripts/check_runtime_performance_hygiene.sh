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
    "phase_lane_set": section_between(
        "pub(crate) fn phase_lane_set(&self, idx: usize)",
        "pub(crate) fn phase_min_start",
    ),
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
  preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  preview_offer_entry_evidence_defers_binding_poll_until_selected_scope \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  nested_dispatch_arm_counts_as_recv_for_known_passive_route \
  --lib \
  --features std
cargo +"${TOOLCHAIN}" test \
  -p hibana \
  global::role_program::tests::lane_set_view_iterates_set_bits_without_empty_lane_scan \
  --lib \
  --features std

echo "runtime performance hygiene check passed"
