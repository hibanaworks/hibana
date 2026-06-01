#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if rg -n 'allow\(dead_code\)|cfg_attr\([^)]*allow\(dead_code\)' src tests integration 2>/dev/null; then
  echo "descriptor streaming hygiene violation: dead_code allow is forbidden" >&2
  exit 1
fi

if rg -n 'Box<|alloc::|std::boxed' src/global/compiled --glob '!**/*tests.rs'; then
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
    echo "descriptor streaming hygiene violation: legacy streaming/materialization owner still present -> ${forbidden_path}" >&2
    exit 1
  fi
done

if rg -n 'with_lowering_lease|RoleLoweringScratchLayout|from_storage|try_init_compiled_role_image_|stream_compiled_role_descriptor_rows|rollback_compiled_role_descriptor_stream|RoleTypestateInitStorage|init_value_from_.*_for_role|stream_value_from_.*_for_role' src/global/compiled src/global/typestate src/global/role_program.rs src/global/role_program >/dev/null; then
  echo "descriptor streaming hygiene violation: transient descriptor streaming path reintroduced" >&2
  exit 1
fi

if rg -n 'write_clone_to|init_lowering: unsafe fn|source\.init_lowering|MaybeUninit::<CompiledProgramImage>' src/global/compiled src/global/role_program.rs src/global/role_program >/dev/null; then
  echo "descriptor streaming hygiene violation: resident program image must be borrowed from the typed projection source, not cloned into attach storage" >&2
  exit 1
fi

if rg -n 'eff_list\.as_slice\(\)' src/global/compiled >/dev/null; then
  echo "descriptor streaming hygiene violation: descriptor query code must not use flat EffList slices" >&2
  exit 1
fi

if rg -n 'view\.as_slice\(\)|CompiledProgramView::as_slice|fn as_slice\(&self\) -> .*\[EffStruct\]' src/global/compiled src/global/typestate >/dev/null; then
  echo "descriptor streaming hygiene violation: resident descriptor query code must stream through segment views, not flat lowering slices" >&2
  exit 1
fi

if rg -n 'pub const fn as_slice\(&self\)' src/global/const_dsl.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: EffList flat view must not be public canonical API" >&2
  exit 1
fi

if rg -n 'impl (core::ops::Deref|AsRef<\[EffStruct\]>) for EffList' src/global/const_dsl.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: EffList must not expose flat slice traits as canonical path" >&2
  exit 1
fi

for required in \
  'src/global/compiled/lowering/driver:segment_len(' \
  'src/global/compiled/lowering/driver:node_at(' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:resident: compiled' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:fn resident_node(' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:fn resident_eff_for_step(' \
  'src/global/compiled/images/image/role_descriptor_ref/route_scope.rs:fn resident_scope_bounds(' \
  'src/global/compiled/images/image/role_descriptor_ref.rs:pub(crate) const fn from_resident(compiled:' \
  'src/g.rs:CompiledRoleImage::new(' \
  'src/g.rs:CompiledProgramRef::resident('
do
  path="${required%%:*}"
  pattern="${required#*:}"
  if ! rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "descriptor streaming hygiene violation: resident descriptor query path missing -> ${required}" >&2
    exit 1
  fi
done

echo "descriptor streaming hygiene check passed"
