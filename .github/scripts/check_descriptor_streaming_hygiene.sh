#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

DEAD_CODE_ALLOW='allow[[:space:]]*\([^]]*dead[_]code'
DEAD_CODE_SPLIT_ALLOW='allow[[:space:]]*\([^]]*dead[[:space:]]*["'\'']?[[:space:]]*[_][[:space:]]*["'\'']?[[:space:]]*code'
check_absent "${DEAD_CODE_ALLOW}|cfg_attr[^\n]*${DEAD_CODE_ALLOW}|${DEAD_CODE_SPLIT_ALLOW}" \
  "dead_code allow is forbidden" \
  src tests .github \
  --glob '!tests/semantic_surface/source_residue_pico_hygiene.rs'

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

check_absent 'Box<|alloc::|std::boxed' \
  "heap-backed descriptor storage in core init path" \
  src/global/compiled
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: heap-backed descriptor storage in core init path" >&2
  exit 1
fi

for forbidden_path in \
  src/global/compiled/materialize \
  src/global/compiled/lowering/program_image_builder.rs \
  src/global/compiled/lowering/program_tail_storage.rs \
  src/global/compiled/lowering/role_image_builder.rs \
  src/global/compiled/lowering/role_image_lowering.rs \
  src/global/compiled/lowering/role_scope_storage.rs
do
  if [[ -e "${forbidden_path}" ]]; then
    echo "descriptor streaming hygiene violation: forbidden streaming/materialization owner still present -> ${forbidden_path}" >&2
    exit 1
  fi
done

check_absent \
  'with_lowering_lease|RoleLoweringScratchLayout|from_storage|try_init_role_image_ref_|stream_compiled_role_descriptor_rows|requeue_compiled_role_descriptor_stream|RoleTypestateInitStorage|init_value_from_.*_for_role|stream_value_from_.*_for_role' \
  "transient descriptor streaming path detected" \
  src/global/compiled src/global/typestate src/global/role_program.rs src/global/role_program
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: transient descriptor streaming path detected" >&2
  exit 1
fi

check_absent \
  'write_clone_to|init_lowering: unsafe fn|source\.init_lowering|MaybeUninit::<CompiledProgramImage>' \
  "resident program image clone path detected" \
  src/global/compiled src/global/role_program.rs src/global/role_program
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: resident program image must be borrowed from the typed projection source, not cloned into attach storage" >&2
  exit 1
fi

check_absent \
  'RoleLaneScratch|MAX_RESIDENT_ROW_BOUNDARY_ROWS|MAX_RESIDENT_LANE_BIT_BYTES|from_scratch|resident_row_at|dependency_for_local_step|dependency_for_event|local_row_contains_lane|local_step_at' \
  "capacity-shaped role projection scratch detected" \
  src/global/role_program
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: role projection must count exact columns then emit directly into final bytes" >&2
  exit 1
fi

check_absent \
  'let[[:space:]]+mut[[:space:]]+[A-Za-z0-9_]+[[:space:]]*=[[:space:]]*\[[^]]*;[[:space:]]*E\]' \
  "event-capacity array in role descriptor plan/emitter detected" \
  src/global/role_program/image_impl/plan.rs \
  src/global/role_program/image_impl/blob_image.rs \
  src/global/role_program/image_impl/projection.rs
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: role descriptor planning memory must not scale with source event capacity" >&2
  exit 1
fi

for required in \
  'src/global/role_program/image_impl/plan.rs:pub(super) const fn from_program<const E: usize>' \
  'src/global/role_program/image_impl/plan.rs:projection::DependencyCursor::new(eff_list, role)' \
  'src/global/role_program/image_impl/plan.rs:projection::ResidentRowCursor::new(eff_list, role)' \
  'src/global/role_program/image_impl/blob_image.rs:pub(crate) const fn emit<const E: usize>' \
  'src/global/role_program/image_impl/blob_image.rs:projection::DependencyCursor::new(eff_list, role)' \
  'src/global/role_program/image_impl/blob_image.rs:projection::ResidentRowCursor::new(eff_list, role)' \
  'src/global/role_program/image_impl/blob_image.rs:out.write_event(columns.events, local_step, event)'
do
  path="${required%%:*}"
  pattern="${required#*:}"
  if ! grep -Fq "${pattern}" "${path}"; then
    echo "descriptor streaming hygiene violation: exact two-pass role emitter authority missing -> ${required}" >&2
    exit 1
  fi
done

check_absent \
  'eff_list\.as_slice\(\)' \
  "flat EffList descriptor query detected" \
  src/global/compiled
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: descriptor query code must not use flat EffList slices" >&2
  exit 1
fi

check_absent \
  'view\.as_slice\(\)|CompiledProgramView::as_slice|fn as_slice\(&self\) -> .*\[EffStruct\]' \
  "flat lowering slice query detected" \
  src/global/compiled src/global/typestate
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: descriptor queries must use indexed event rows, not flat lowering slices" >&2
  exit 1
fi

check_absent \
  'pub const fn as_slice\(&self\)' \
  "public EffList flat view detected" \
  src/global/const_dsl.rs
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: EffList flat view must not be public canonical API" >&2
  exit 1
fi

check_absent \
  'impl (core::ops::Deref|AsRef<\[EffStruct\]>) for EffList' \
  "flat EffList trait view detected" \
  src/global/const_dsl.rs
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: EffList must not expose flat slice traits as canonical path" >&2
  exit 1
fi

for required in \
  'src/global/const_dsl/eff_list.rs:pub(crate) const fn node_at' \
  'src/global/compiled/lowering/driver/impls/image.rs:pub(crate) const fn scan_const' \
  'src/global/compiled/lowering/driver/impls/image.rs:Self::scan_impl(eff_list)' \
  'src/g.rs:SOURCE_EFF_LIST' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:resident: image' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:pub(crate) const fn from_resident(image:' \
  'src/g/role_projection.rs:const IMAGE_REF: crate::global::role_program::RoleImageRef' \
  'src/g/role_projection.rs:ProgramProjection::<Steps, CAPACITY>::PROGRAM_REF' \
  'src/global/compiled/images/image/blob_storage.rs:CompiledProgramRef::compact('
do
  path="${required%%:*}"
  pattern="${required#*:}"
  if [[ ! -e "${path}" ]]; then
    echo "descriptor streaming hygiene violation: resident descriptor query path owner missing -> ${path}" >&2
    exit 1
  fi
  if [[ -d "${path}" ]]; then
    if ! grep -R -Fq "${pattern}" "${path}"; then
      echo "descriptor streaming hygiene violation: resident descriptor query path missing -> ${required}" >&2
      exit 1
    fi
  elif ! grep -Fq "${pattern}" "${path}"; then
    echo "descriptor streaming hygiene violation: resident descriptor query path missing -> ${required}" >&2
    exit 1
  fi
done

if [[ -e src/global/compiled/images/image/role_descriptor_ref/tests/route_scope.rs ]]; then
  echo "descriptor streaming hygiene violation: RoleDescriptorRef must not keep a test-only lowering route-scope helper module" >&2
  exit 1
fi

check_absent \
  'fn resident_node\(|fn resident_eff_for_step\(|program_image\(\)\.view\(\)' \
  "RoleDescriptorRef lowering scratch rebuild path detected" \
  src/global/compiled/images/image/role_descriptor_ref.rs
if [[ "${FAILED}" -ne 0 ]]; then
  echo "descriptor streaming hygiene violation: RoleDescriptorRef must not rebuild nodes from lowering scratch" >&2
  exit 1
fi

echo "descriptor streaming hygiene check passed"
