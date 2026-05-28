#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" --glob '!src/endpoint/kernel/test_support/**' "$@"; then
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

for forbidden_path in \
  src/global/compiled/layout.rs \
  src/global/compiled/materialize \
  src/global/compiled/lowering/program_image_builder.rs \
  src/global/compiled/lowering/program_tail_storage.rs \
  src/global/compiled/lowering/role_image_builder.rs \
  src/global/compiled/lowering/role_image_lowering.rs \
  src/global/compiled/lowering/role_scope_storage.rs \
  src/global/typestate/builder.rs \
  src/global/typestate/emit.rs \
  src/global/typestate/emit_route.rs \
  src/global/typestate/emit_scope.rs \
  src/global/typestate/emit_walk.rs \
  src/global/typestate/registry.rs \
  src/global/typestate/route_facts.rs
do
  if [[ -e "${forbidden_path}" ]]; then
    echo "exact layout hygiene violation: legacy exact-layout owner still present -> ${forbidden_path}" >&2
    FAILED=1
  fi
done

check_absent \
  "RoleImageLayoutInput|ProgramLayoutFacts|RoleLayoutFacts" \
  "legacy split role layout owners reintroduced" \
  src

check_absent \
  "EndpointArenaLayout::new\\(" \
  "legacy multi-argument endpoint arena layout constructor reintroduced" \
  src

check_absent \
  "PUBLIC_ATTACH_TAIL_FLOOR|IMAGE_BANK_EXPANSION_TAIL_FLOOR|PUBLIC_ENDPOINT_ATTACH_TAIL_FLOOR" \
  "public endpoint storage must be sized from resident role footprints, not tail-floor heuristics" \
  src/rendezvous/core.rs

check_absent \
  "MAX_EFF_NODES" \
  "runtime layout must not reserve endpoint storage from global effect-node ceilings" \
  src/rendezvous/core.rs src/endpoint src/control \
  --glob '!**/*tests.rs'

check_absent \
  "RoleCompileScratch|ROLE_COMPILE_SCRATCH_MAX_|OFFER_TEST_LANE_CAPACITY|TEST_ENDPOINT_LANE_CAPACITY|TEST_LANE_SNAPSHOT_CAPACITY|BindingInboxTestArena|FrontierObservationKeyTestArena|ObservedKeyTestArena|CachedSpliceOperandsMap|with_test_lane_set\\(|FixtureHarness|run::<scenario::FixtureHarness>" \
  "test-only exact-world cleanup must not regress to renamed ceilings, shared arenas, low-lane helpers, or compile-only shadow harnesses" \
  src tests \
  --glob '!tests/public_surface_guards.rs'

check_absent \
  "while lane_idx < MAX_LANES|if lane_idx >= MAX_LANES|while logical_idx < MAX_LANES" \
  "runtime hot paths must not regress to fixed MAX_LANES lane scans" \
  src/endpoint src/rendezvous src/control src/global/typestate

check_absent \
  "active_route_lane_mask: RoleLaneMask|lane_linger_mask: RoleLaneMask|lane_offer_linger_mask: RoleLaneMask|active_offer_mask: RoleLaneMask|nonempty_mask: RoleLaneMask|observed_offer_lane_mask: RoleLaneMask|global_frontier_observed_offer_lane_mask: RoleLaneMask" \
  "runtime sidecars must not retain scalar lane-mask cache state" \
  src/endpoint

check_absent \
  "RoleLoweringScratch|LoweringLeaseMode|with_lowering_lease|MaterializedRoleImage|RoleImageSlice::from_raw\\(|CompiledProgramRef::from_raw\\(" \
  "attach/runtime layout must not keep lowering scratch or raw materialization fallbacks" \
  src

check_required \
  "pub(crate) struct RoleFootprint {" \
  "RoleFootprint owner missing" \
  src/global/role_program/image_types.rs

check_required \
  "words: [u16; 14]," \
  "RoleFacts must stay a compact word array" \
  src/global/role_program/image_types.rs

check_required \
  "pub(crate) const fn footprint(self) -> RoleFootprint {" \
  "RoleImageRef must expose the resident role footprint" \
  src/global/role_program/image_impl.rs

check_required \
  "pub(crate) struct RouteFrontierArenaLayout {" \
  "RouteFrontierArenaLayout owner missing" \
  src/endpoint/kernel/layout.rs

check_required \
  "pub(crate) const fn from_footprint_with_binding(" \
  "EndpointArenaLayout must be constructed from RoleFootprint" \
  src/endpoint/kernel/layout.rs

check_required \
  "fn endpoint_layout_footprint(" \
  "resident role descriptor must derive endpoint layout from a footprint owner" \
  src/global/compiled/images/image/role_descriptor_ref.rs

check_required \
  "pub(crate) fn endpoint_arena_layout_for_binding(" \
  "resident role descriptor must expose endpoint arena layout without lowering scratch" \
  src/global/compiled/images/image/role_descriptor_ref.rs

check_required \
  "pub(crate) const fn frontier_workspace_guard_bytes(" \
  "rendezvous must size frontier workspace from resident endpoint arena layout" \
  src/rendezvous/core/storage_layout.rs

check_required \
  "frontier_workspace_bytes" \
  "rendezvous/port split must carry descriptor-derived frontier workspace size" \
  src/rendezvous/core.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "exact layout hygiene check passed"
