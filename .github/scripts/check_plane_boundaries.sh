#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

# Mgmt ordinary-prefix redesign forbidden the direct runtime driver/automaton owners.
check_absent_multiline "cluster[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*drive_mgmt\\(" "forbidden drive_mgmt hook returned" src
check_absent_multiline "\\bmgmt_managers\\b" "cluster mgmt manager cache returned" src/session/cluster/core.rs
check_absent_multiline "\\bon_decision_boundary\\(" "cluster decision-boundary mgmt hook returned" src/session/cluster/core.rs

# Direct manager mutators must not survive in production rendezvous helpers.
check_absent_multiline "manager[[:space:][:cntrl:]]*\\.[[:space:][:cntrl:]]*(load_commit|schedule_activate|on_decision_boundary|revert)\\(" "forbidden rendezvous resolver mutator owner returned" src/rendezvous/core.rs

# Slot-bundle wrapper helpers were forbidden with the mgmt automaton path.
check_absent_multiline "fn (load_commit_with|schedule_activate_with|on_decision_boundary_for_slot_with|revert_with)\\b" "forbidden slot-bundle wrapper returned" src/rendezvous/core.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "plane boundary check passed"
