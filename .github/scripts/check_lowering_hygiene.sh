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
    echo "lowering hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent_outside() {
  local pattern="$1"
  local label="$2"
  shift 2
  local globs=()
  local exclude
  for exclude in "$@"; do
    globs+=("-g" "!${exclude}")
  done
  if rg -n -U "${pattern}" src "${globs[@]}"; then
    echo "lowering hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent \
  "interpret_eff_list\\(" \
  "legacy interpret_eff_list lowering shim" \
  src

check_absent \
  "\\.policies\\(" \
  "direct EffList policy-marker scan" \
  src

check_absent \
  "pub[[:space:]]+use[[:space:]].*EffList" \
  "EffList leaked through a public export" \
  src

check_absent \
  "pub[[:space:]]+const[[:space:]]+fn[[:space:]]+eff_list\\(" \
  "RoleProgram public eff_list accessor reintroduced" \
  src/global/role_program.rs

check_absent \
  "fn[[:space:]]+machine\\(" \
  "RoleProgram machine owner reintroduced" \
  src/global/role_program.rs

check_absent \
  "fn[[:space:]]+lease_budget\\(" \
  "RoleProgram lease budget accessor reintroduced" \
  src/global/role_program.rs

check_absent \
  "PhaseCursor::from_machine" \
  "PhaseCursor::from_machine reintroduced" \
  src

check_absent \
  "ProgramStamp::from_eff_list" \
  "separate ProgramStamp raw-scan helper reintroduced" \
  src

check_absent_outside \
  "from_eff_list\\(" \
  "raw EffList lowering helper used outside ProjectionSeal" \
  "src/global/compiled/seal.rs"

check_absent_outside \
  "CompiledProgram::compile\\(" \
  "direct CompiledProgram::compile call used outside compiled owners or test-only fixtures" \
  "src/global/compiled/program.rs" \
  "src/control/cluster/effects.rs" \
  "src/runtime/mgmt/kernel.rs"

check_absent_outside \
  "CompiledRole::compile\\(" \
  "direct CompiledRole::compile call used outside compiled owners or test-only fixtures" \
  "src/global/compiled/role.rs" \
  "src/global/role_program.rs" \
  "src/endpoint/kernel/core.rs"

check_absent \
  "(enum[[:space:]]+DynamicLabelClass|fn[[:space:]]+(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\()|\\b(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\(" \
  "deprecated endpoint raw-label semantic helpers reintroduced" \
  src

check_absent \
  "macro_rules![[:space:]]+impl_control_resource" \
  "public impl_control_resource macro reintroduced" \
  src

if [[ -e "src/global/compiled/facts.rs" || -e "src/global/compiled/machine.rs" ]]; then
  echo "lowering hygiene violation: legacy compiled owners still present on disk" >&2
  FAILED=1
fi

if [[ ! -d "src/endpoint/kernel" ]]; then
  echo "lowering hygiene violation: src/endpoint/kernel split owner missing" >&2
  FAILED=1
fi

if [[ -e "src/endpoint/cursor.rs" ]]; then
  echo "lowering hygiene violation: legacy endpoint/cursor.rs owner still present" >&2
  FAILED=1
fi

while IFS= read -r hit; do
  [[ -z "${hit}" ]] && continue
  case "${hit}" in
    *"src/control/cap/resource_kinds.rs:"*"macro_rules! define_control_resource_kind"*) ;;
    *"src/control/cap/resource_kinds.rs:"*"macro_rules! decode_mask"*) ;;
    *"src/control/cluster/core.rs:"*"macro_rules! mask_for"*) ;;
    *"src/global/steps.rs:"*"macro_rules! impl_role_eq"*) ;;
    *"src/transport/wire.rs:"*"macro_rules! impl_wire_for_int"*) ;;
    *"src/transport/wire.rs:"*"macro_rules! push"*) ;;
    *)
      echo "lowering hygiene violation: new macro_rules! owner detected -> ${hit}" >&2
      FAILED=1
      ;;
  esac
done < <(rg -n "macro_rules![[:space:]]+[A-Za-z_][A-Za-z0-9_]*" src)

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "lowering hygiene check passed"
