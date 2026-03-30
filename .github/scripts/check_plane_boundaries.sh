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
    echo "boundary violation: ${label}" >&2
    FAILED=1
  fi
}

# Mgmt ordinary-prefix redesign removed the direct runtime driver/automaton owners.
check_absent "fn apply_seed\\b" "legacy apply_seed owner returned" src/runtime/mgmt.rs
check_absent "cluster[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*drive_mgmt\\(" "legacy drive_mgmt hook returned" src
check_absent "\\bmgmt_managers\\b" "cluster mgmt manager cache returned" src/control/cluster/core.rs
check_absent "\\bon_decision_boundary\\(" "cluster decision-boundary mgmt hook returned" src/control/cluster/core.rs

# Direct manager mutators must not survive in production mgmt helpers.
check_absent "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(load_begin|load_chunk)\\(" "legacy mgmt load mutator owner returned" src/runtime/mgmt.rs src/runtime/mgmt/request_reply.rs src/runtime/mgmt/observe_stream.rs
check_absent "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(load_commit|schedule_activate|on_decision_boundary|revert)\\(" "legacy rendezvous policy mutator owner returned" src/rendezvous/core.rs

# Slot-bundle wrapper helpers were removed with the mgmt automaton path.
check_absent "fn (load_commit_with|schedule_activate_with|on_decision_boundary_for_slot_with|revert_with)\\b" "legacy slot-bundle wrapper returned" src/rendezvous/core.rs

# Runtime mgmt helper family must stay deleted.
check_absent "fn (enter_controller|enter_cluster|enter_stream_controller|enter_stream_cluster|drive_controller|drive_cluster|drive_stream_cluster|drive_stream_controller)\\b" "legacy runtime mgmt helper returned" src/runtime/mgmt.rs src/runtime/mgmt/request_reply.rs src/runtime/mgmt/observe_stream.rs src/runtime/mgmt/test_support.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "plane boundary check passed"
