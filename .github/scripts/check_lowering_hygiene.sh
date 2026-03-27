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

check_absent_outside \
  "(matches!\\([^\\n]*LABEL_LOOP_CONTINUE[^\\n]*LABEL_LOOP_BREAK|LoopDisposition::Continue[[:space:]]*=>[[:space:]]*LABEL_LOOP_CONTINUE|LoopDisposition::Break[[:space:]]*=>[[:space:]]*LABEL_LOOP_BREAK|0[[:space:]]*=>[[:space:]]*LABEL_LOOP_CONTINUE|1[[:space:]]*=>[[:space:]]*LABEL_LOOP_BREAK|==[[:space:]]*Some\\(LABEL_LOOP_CONTINUE\\)|==[[:space:]]*Some\\(LABEL_LOOP_BREAK\\)|==[[:space:]]*LABEL_LOOP_CONTINUE|==[[:space:]]*LABEL_LOOP_BREAK)" \
  "label-based loop meaning check outside semantic helper owners" \
  "src/endpoint/cursor.rs" \
  "src/global/const_dsl.rs" \
  "src/global/typestate.rs"

check_absent \
  "fn[[:space:]]+wire_label_for_controller_arm\\(" \
  "endpoint wire-label wrapper reintroduced" \
  src/endpoint/cursor.rs

check_absent \
  "fn[[:space:]]+controller_arm_loop_control\\(" \
  "endpoint loop-control wrapper reintroduced" \
  src/endpoint/cursor.rs

check_absent_outside \
  "(enum[[:space:]]+DynamicLabelClass|fn[[:space:]]+(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\()" \
  "endpoint wire-semantic adapter defined outside canonical seam owner" \
  "src/endpoint/cursor.rs"

check_absent_outside \
  "(controller_arm_loop_meaning|controller_arm_wire_label|loop_control_meaning_from_wire_label|wire_label_for_loop_control|classify_dynamic_label)\\(" \
  "endpoint wire-semantic adapter used outside canonical seam owner" \
  "src/endpoint/cursor.rs"

while IFS= read -r hit; do
  [[ -z "${hit}" ]] && continue
  case "${hit}" in
    *"src/control/cap/resource_kinds.rs:"*"macro_rules! impl_control_resource"*) ;;
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
