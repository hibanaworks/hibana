#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

check_absent_multiline \
  "\\bProgramFacts\\b" \
  "forbidden ProgramFacts owner or vocabulary detected" \
  src README.md tests

check_absent_multiline \
  "budget_for_role_program\\(" \
  "forbidden role-program budget rescan helper detected" \
  src

check_absent_multiline \
  "interpret_eff_list\\(" \
  "forbidden interpret_eff_list lowering forbidden path" \
  src

check_absent_multiline \
  "\\.policies\\(" \
  "direct EffList resolver-marker scan" \
  src

check_absent_multiline \
  "pub[[:space:]]+use[[:space:]].*EffList" \
  "EffList leaked through a public export" \
  src

check_absent_multiline \
  "\\bEffList\\b" \
  "runtime/session layer mentions raw EffList instead of compiled facts" \
  src/session src/endpoint src/rendezvous src/transport.rs

check_absent_multiline \
  "pub[[:space:]]+const[[:space:]]+fn[[:space:]]+eff_list\\(" \
  "RoleProgram public eff_list accessor detected" \
  src/global/role_program.rs

check_absent_multiline \
  "fn[[:space:]]+machine\\(" \
  "RoleProgram machine owner detected" \
  src/global/role_program.rs

check_absent_multiline \
  "fn[[:space:]]+lease_budget\\(" \
  "RoleProgram lease budget accessor detected" \
  src/global/role_program.rs

check_absent_multiline \
  "EventCursor::from_machine" \
  "EventCursor::from_machine detected" \
  src

PROGRAM_STAMP_PATTERN='ProgramStamp'
check_absent_multiline \
  "\\b${PROGRAM_STAMP_PATTERN}\\b" \
  "production program stamp/debug identity metadata detected" \
  src

check_absent_multiline \
  "panic_repo_test\\(|pub\\([^)]*\\)[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(segment_summary|resolver_at|node_at)\\(" \
  "lowering production owners must not retain test-only wrapper accessors" \
  src/global/compiled/lowering

check_absent_multiline \
  "cfg_attr\\(test" \
  "test-only derives detected on resident cursor state" \
  src/global/typestate/cursor.rs

check_absent_outside_tests \
  "from_eff_list\\(" \
  "raw EffList lowering helper used outside compiled lowering seal" \
  "src/global/compiled/seal.rs"

check_absent_outside_tests \
  "CompiledProgram::compile\\(" \
  "direct CompiledProgram::compile call used outside compiled owners or test-only test support" \
  "src/global/compiled/program.rs" \
  "src/session/cluster/effects.rs"

check_absent_outside_tests \
  "CompiledRole::compile\\(" \
  "direct CompiledRole::compile call used outside compiled owners or test-only test support" \
  "src/global/compiled/role.rs" \
  "src/global/role_program.rs" \
  "src/endpoint/kernel/core.rs"

check_absent_multiline \
  "(enum[[:space:]]+DynamicLabelClass|fn[[:space:]]+(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\()|\\b(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\(" \
  "forbidden endpoint raw-label semantic helpers detected" \
  src

check_absent_multiline \
  "macro_rules![[:space:]]+impl_control_resource" \
  "public impl_control_resource macro detected" \
  src

check_absent_multiline \
  '#\[path[[:space:]]*=[[:space:]]*"../lowering/' \
  "frozen image owner path-imports lowering modules directly" \
  src/global/compiled/images

check_absent_multiline \
  "use[[:space:]]+crate::global::compiled::lowering::" \
  "frozen image owner imports lowering helpers directly" \
  src/global/compiled/images/program.rs

check_absent_multiline \
  "struct[[:space:]]+CompiledProgramTailStorage[[:space:]]*\\{" \
  "pointer-rich compiled-program storage leaked back into frozen image owner" \
  src/global/compiled/images/program.rs

check_absent_multiline \
  "impl[[:space:]]+CompiledProgramTailStorage[[:space:]]*\\{" \
  "compiled-program lowering impl leaked back into frozen image owner" \
  src/global/compiled/images/program.rs

check_absent_multiline \
  "pub\\(crate\\)[[:space:]]+use[[:space:]]+image_builder::init_compiled_program_image" \
  "compiled-program image builder re-export leaked back into frozen image owner" \
  src/global/compiled/images/program.rs

check_absent_multiline \
  "struct[[:space:]]+CompiledProgram[[:space:]]*\\{|impl[[:space:]]+CompiledProgram[[:space:]]*\\{" \
  "compiled-program test/build owner leaked back into frozen image owner" \
  src/global/compiled/images/program.rs

check_absent_multiline \
  "^(fn|const fn|unsafe fn|pub\\([^)]*\\)[[:space:]]+fn|pub\\([^)]*\\)[[:space:]]+const[[:space:]]+fn|pub\\([^)]*\\)[[:space:]]+unsafe[[:space:]]+fn)[[:space:]]+(compiled_program_push_dynamic_resolver_site|compiled_program_push_resource|compiled_program_route_scope_end|compiled_program_insert_route_resolver|compiled_program_emit_route_resolvers|compiled_program_emit_atom_into_slices|compiled_program_emit_atom|control_scope_mask_bit)\\(" \
  "compiled-program lowering helpers leaked back into frozen image owner" \
  src/global/compiled/images/program.rs

DELETED_COMPILED_ROLE_OWNER="src/global/compiled/images/role.rs"
if [[ -e "${DELETED_COMPILED_ROLE_OWNER}" ]]; then
  echo "${DELETED_COMPILED_ROLE_OWNER}" >&2
  echo "lowering hygiene violation: forbidden compiled-role owner detected" >&2
  FAILED=1
fi

if [[ -e "src/global/compiled/facts.rs" || -e "src/global/compiled/machine.rs" ]]; then
  echo "lowering hygiene violation: forbidden compiled owners still present on disk" >&2
  FAILED=1
fi

if [[ ! -d "src/endpoint/kernel" ]]; then
  echo "lowering hygiene violation: src/endpoint/kernel split owner missing" >&2
  FAILED=1
fi

if [[ -e "src/endpoint/cursor.rs" ]]; then
  echo "lowering hygiene violation: forbidden endpoint/cursor.rs owner still present" >&2
  FAILED=1
fi

for forbidden_path in \
  src/global/typestate/emit.rs \
  src/global/typestate/emit_walk.rs \
  src/global/typestate/emit_scope.rs \
  src/global/typestate/emit_route.rs \
  src/global/typestate/builder.rs \
  src/global/typestate/registry.rs \
  src/global/typestate/route_facts.rs \
  src/global/compiled/layout.rs \
  src/global/compiled/materialize \
  src/global/compiled/lowering/program_image_builder.rs \
  src/global/compiled/lowering/program_tail_storage.rs \
  src/global/compiled/lowering/role_image_builder.rs \
  src/global/compiled/lowering/role_image_lowering.rs \
  src/global/compiled/lowering/role_scope_storage.rs
do
  if [[ -e "${forbidden_path}" ]]; then
    echo "lowering hygiene violation: forbidden lowering/typestate owner still present -> ${forbidden_path}" >&2
    FAILED=1
  fi
done

for required in \
  'src/global/role_program/image_types.rs:pub(crate) struct RoleImageRef' \
  'src/global/role_program/image_types.rs:pub(crate) struct RuntimeRoleFacts' \
  'src/g/role_projection.rs:const IMAGE_REF: crate::global::role_program::RoleImageRef' \
  'src/g/role_projection.rs:&RoleProjection::<ROLE, Steps>::IMAGE_REF' \
  'src/g/role_projection.rs:ProgramImageBytes' \
  'src/g/role_projection.rs:ProgramProjection::<Steps>::PROGRAM_REF' \
  "src/global/role_program/program.rs:image: &'static crate::global::role_program::RoleImageRef" \
  'src/global/compiled/images/image/role_descriptor_ref.rs:resident: image' \
  'src/session/cluster/core/session_cluster_ops.rs:RoleImageSlice::from_resident(compiled)' \
  'src/session/cluster/core/session_cluster_ops.rs:program.role_image_ref().program'
do
  path="${required%%:*}"
  pattern="${required#*:}"
  check_required "${pattern}" \
    "resident descriptor owner missing -> ${required}" \
    "${path}"
done

capture_rg LOWERING_MACRO_HITS \
  "macro_rules owner scan" \
  -n "macro_rules![[:space:]]+[A-Za-z_][A-Za-z0-9_]*" src \
  -g '!src/**/kani.rs' \
  -g '!src/**/tests/**' \
  -g '!src/**/tests.rs'
while IFS= read -r hit; do
  [[ -z "${hit}" ]] && continue
  case "${hit}" in
    *"src/session/cluster/core.rs:"*"macro_rules! mask_for"*) ;;
    *"src/transport/wire.rs:"*"macro_rules! impl_wire_for_int"*) ;;
    *"src/transport/wire.rs:"*"macro_rules! push"*) ;;
    *)
      echo "lowering hygiene violation: new macro_rules! owner detected -> ${hit}" >&2
      FAILED=1
      ;;
  esac
done <<< "${LOWERING_MACRO_HITS}"

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "lowering hygiene check passed"
