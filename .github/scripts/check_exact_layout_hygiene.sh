#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "exact layout hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_required() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  if ! rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "exact layout hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent \
  "RoleImageLayoutInput|ProgramLayoutFacts|RoleLayoutFacts" \
  "legacy split role layout owners reintroduced" \
  src

check_absent \
  "EndpointArenaLayout::new\\(" \
  "legacy multi-argument endpoint arena layout constructor reintroduced" \
  src

check_absent \
  "phases: \\*const Phase" \
  "legacy compiled role phase-array image reintroduced" \
  src/global/compiled/images/role.rs

check_absent \
  "active_lane_mask_bits|active_lane_count_from_mask\\(" \
  "compiled role image must not regress to mask-derived active lane facts" \
  src/global/compiled/images/role.rs

check_absent \
  "lane_entry_len: u8|lane_mask: u8" \
  "compiled role exact phase image must not regress to u8 lane-mask ceilings" \
  src/global/compiled/images/role.rs

check_absent \
  "MAX_STATES" \
  "compiled role image must not clamp typestate storage with MAX_STATES" \
  src/global/compiled/images/role.rs

check_absent \
  "MAX_PHASES|MAX_STEPS|MAX_LANES" \
  "exact lowering validation must not reintroduce fixed MAX_* caps" \
  src/global/compiled/lowering/seal.rs \
  src/global/compiled/materialize/lease.rs

check_absent \
  "role_program::\\{MAX_LANES, Phase\\}" \
  "phase cursor must not depend on whole Phase values" \
  src/global/typestate/cursor.rs

check_absent \
  "lane_cursors: \\[u16; MAX_LANES\\]|current_step_labels: \\[u8; MAX_LANES\\]" \
  "phase cursor state must not regress to fixed MAX_LANES cursor arrays" \
  src/global/typestate/cursor.rs

check_absent \
  "lane_route_arm_lens: \\[u8; MAX_LANES\\]|lane_linger_counts: \\[u8; MAX_LANES\\]|lane_dense_by_lane: &\\[u8; MAX_LANES\\]" \
  "route state must keep lane bookkeeping in exact sidecar storage" \
  src/endpoint/kernel/runtime/route_state.rs

check_absent \
  "struct DenseLaneIndex \\{\\n\\s+lane_dense_by_lane: \\[u8; MAX_LANES\\],|lane_dense_by_lane: &\\[u8; MAX_LANES\\]" \
  "binding inbox dense-lane index must stay pointer-backed in non-test code" \
  src/endpoint/kernel/runtime/inbox.rs

check_absent \
  "\\[ScopeId; MAX_ROUTE_ARM_STACK\\]|collect_lane_scopes\\(" \
  "route runtime must not reintroduce fixed MAX_ROUTE_ARM_STACK stack scratch collectors" \
  src/endpoint/kernel/core.rs \
  src/endpoint/kernel/runtime/route_state.rs

check_absent \
  "while lane_idx < MAX_LANES" \
  "route runtime hot paths in core.rs must not regress to full MAX_LANES scans" \
  src/endpoint/kernel/core.rs

check_absent \
  "while lane_idx < MAX_LANES" \
  "typestate cursor hot paths must not regress to full MAX_LANES scans" \
  src/global/typestate/cursor.rs

check_absent \
  "if lane_idx >= MAX_LANES" \
  "typestate cursor lane guards must size against exact logical-lane counts" \
  src/global/typestate/cursor.rs

check_absent \
  "while lane_idx < MAX_LANES" \
  "binding inbox hot paths must not regress to full MAX_LANES scans" \
  src/endpoint/kernel/runtime/inbox.rs

check_absent \
  "lane_first_eff: \\[EffIndex; MAX_LANES\\]|lane_last_eff: \\[EffIndex; MAX_LANES\\]|arm0_lane_last_eff: \\[EffIndex; MAX_LANES\\]" \
  "typestate registry lane facts must stay in exact sidecar matrices" \
  src/global/typestate/registry.rs

check_absent \
  "if lane_idx >= MAX_LANES|\\(preview_lane as usize\\) < MAX_LANES" \
  "core route runtime must bound lanes with exact logical-lane counts" \
  src/endpoint/kernel/core.rs

check_absent \
  "use crate::global::role_program::MAX_LANES|0\\.\\.crate::global::role_program::MAX_LANES|while logical_idx < MAX_LANES" \
  "session-cluster lane leasing must use exact compiled lane counts" \
  src/control/cluster/core.rs

check_absent \
  "if lane_idx >= MAX_LANES|while lane_idx < MAX_LANES" \
  "route frontier selection/refresh owners must not regress to fixed MAX_LANES lane bounds" \
  src/endpoint/kernel/route_frontier/frontier_select.rs \
  src/endpoint/kernel/route_frontier/offer_refresh.rs \
  src/endpoint/kernel/route_frontier/offer.rs

check_absent \
  "summary_lane_idx >= MAX_LANES|preferred_lane_idx < MAX_LANES" \
  "route frontier scope-evidence helpers must not depend on the fixed MAX_LANES ceiling" \
  src/endpoint/kernel/route_frontier/scope_evidence_logic.rs

check_absent \
  "allocated_slots\\(" \
  "route state must size route-arm storage from exact active-lane counts, not by rescanning lane maps" \
  src/endpoint/kernel/runtime/route_state.rs

check_required \
  "pub(crate) struct RoleFootprint {" \
  "RoleFootprint owner missing" \
  src/global/role_program.rs

check_required \
  "pub(crate) const fn footprint(&self) -> RoleFootprint {" \
  "RoleLoweringInput must expose RoleFootprint" \
  src/global/role_program.rs

check_required \
  "pub(crate) struct RouteFrontierArenaLayout {" \
  "RouteFrontierArenaLayout owner missing" \
  src/endpoint/kernel/runtime/layout.rs

check_required \
  "pub(crate) const fn from_footprint(footprint: RoleFootprint) -> Self {" \
  "EndpointArenaLayout must be constructed from RoleFootprint" \
  src/endpoint/kernel/runtime/layout.rs

check_required \
  "fn endpoint_layout_footprint(" \
  "CompiledRoleImage must derive endpoint layout from a footprint owner" \
  src/global/compiled/images/role.rs

check_required \
  "struct PhaseImageHeader {" \
  "compiled role exact phase-header owner missing" \
  src/global/compiled/images/role.rs

check_required \
  "struct PhaseLaneEntry {" \
  "compiled role exact phase lane-entry owner missing" \
  src/global/compiled/images/role.rs

check_required \
  "pub(crate) struct RoleLoweringScratch<'a> {" \
  "lowering lease exact scratch owner missing" \
  src/global/compiled/materialize/lease.rs

check_required \
  "RoleTypestateBuildScratch" \
  "lowering lease must bind typestate build scratch directly" \
  src/global/compiled/materialize/lease.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "exact layout hygiene check passed"
