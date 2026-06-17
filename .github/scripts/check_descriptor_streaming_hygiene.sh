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
  echo "descriptor streaming hygiene violation: resident descriptor query code must stream through segment views, not flat lowering slices" >&2
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
  'src/global/compiled/lowering/driver:segment_len(' \
  'src/global/compiled/lowering/driver:node_at(' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:resident: image' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:pub(crate) const fn from_resident(image:' \
  'src/g/role_projection.rs:const IMAGE_REF: crate::global::role_program::RoleImageRef' \
  'src/g/role_projection.rs:ProgramProjection::<Steps>::PROGRAM_REF' \
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
