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
