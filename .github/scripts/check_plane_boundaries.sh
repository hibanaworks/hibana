#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_required_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  if ! rg -n -U "${pattern}" "$@" >/dev/null; then
    echo "boundary violation: missing canonical ${label}" >&2
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
    echo "boundary violation: ${label}" >&2
    FAILED=1
  fi
}

# Direct drive_mgmt usage must stay on the canonical apply_seed boundary.
check_required_multiline \
  "(?s)fn apply_seed[^\\{]*\\{.*?cluster[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*drive_mgmt\\(" \
  "apply_seed drive_mgmt owner" \
  src/runtime/mgmt.rs
check_absent_outside \
  "cluster[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*drive_mgmt\\(" \
  "direct drive_mgmt call outside apply_seed owner" \
  src/runtime/mgmt.rs

# Direct load mutators must stay inside the management kernel load branch.
check_required_multiline \
  "(?s)async fn drive_load_branch[^\\{]*\\{.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*load_begin\\(.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*load_chunk\\(" \
  "drive_load_branch load mutator owner" \
  src/runtime/mgmt/kernel.rs
check_absent_outside \
  "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(load_begin|load_chunk)\\(" \
  "direct load mutator outside mgmt kernel/test owner" \
  src/runtime/mgmt/kernel.rs \
  src/runtime/mgmt.rs

# Rendezvous slot-bundle helpers own the remaining policy mutators.
check_required_multiline \
  "(?s)fn load_commit_with[^\\{]*\\{.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*load_commit\\(" \
  "slot-bundle load_commit owner" \
  src/rendezvous/core.rs
check_required_multiline \
  "(?s)fn schedule_activate_with[^\\{]*\\{.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*schedule_activate\\(" \
  "slot-bundle schedule_activate owner" \
  src/rendezvous/core.rs
check_required_multiline \
  "(?s)fn on_decision_boundary_for_slot_with[^\\{]*\\{.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*on_decision_boundary\\(" \
  "slot-bundle decision-boundary owner" \
  src/rendezvous/core.rs
check_required_multiline \
  "(?s)fn revert_with[^\\{]*\\{.*?manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*revert\\(" \
  "slot-bundle revert owner" \
  src/rendezvous/core.rs
check_absent_outside \
  "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(load_commit|schedule_activate|on_decision_boundary|revert)\\(" \
  "direct rendezvous policy mutator outside slot-bundle/test owner" \
  src/rendezvous/core.rs \
  src/runtime/mgmt.rs
check_absent_outside \
  "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(set_policy_mode|set_policy_mode_staged)\\(" \
  "direct policy-mode mutator outside manager tests" \
  src/runtime/mgmt.rs

# Direct seed_from_code usage is forbidden outside substrate::mgmt::session/mgmt internals.
while IFS= read -r hit; do
  [[ -z "${hit}" ]] && continue
  file="${hit%%:*}"
  case "${file}" in
      src/runtime/mgmt.rs)
        ;;
    *)
      echo "boundary violation: direct seed_from_code usage outside session API -> ${hit}" >&2
      FAILED=1
      ;;
  esac
done < <(rg -n "seed_from_code\\(" src tests examples)

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "plane boundary check passed"
