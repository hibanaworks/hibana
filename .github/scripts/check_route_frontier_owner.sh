#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

fail() {
  echo "route frontier owner violation: $1" >&2
  FAILED=1
}

check_file() {
  local path="$1"
  [[ -s "${path}" ]] || fail "missing owner file ${path}"
}

owner_manifest=".github/maintainability/route_frontier_owner_files.txt"
while IFS= read -r owner; do
  [[ -z "${owner}" || "${owner}" == \#* ]] && continue
  check_file "${owner}"
done < "${owner_manifest}"

check_absent_multiline \
  "transport_payload_len|transport_payload_lane|ProbeBinding \\{" \
  "offer frontier regressed to implicit payload cache or probe-owned state" \
  src/endpoint/kernel/core

check_absent_multiline \
  "ingress_evidence: \\[Option<|transport_payload: \\[Option<" \
  "offer restore regressed to anonymous mini-vec ownership" \
  src/endpoint/kernel/offer.rs \
  src/endpoint/kernel/offer/state.rs

check_absent_multiline \
  "lane_route_arms:|root_frontier_state:|offer_entry_state:|scope_evidence:" \
  "core.rs reabsorbed split endpoint state owners" \
  src/endpoint/kernel/core.rs

check_absent_multiline \
  "lane_route_arms\\[[^]]+\\][[:space:]]*=|lane_reentry_counts\\[[^]]+\\][[:space:]]*=|lane_offer_state\\[[^]]+\\][[:space:]]*=" \
  "core.rs detected direct route-state table mutation" \
  src/endpoint/kernel/core.rs

check_absent_multiline \
  "offer_entry_state\\[[^]]+\\][[:space:]]*=|offer_entry_state\\.get_mut\\(|global_active_entries\\.(insert_entry|remove_entry)" \
  "core.rs detected direct frontier table mutation" \
  src/endpoint/kernel/core.rs

check_absent_multiline \
  "root_frontier_state\\[[^]]+\\][[:space:]]*=|global_frontier_observed(_generation|_key)?[[:space:]]*=|global_offer_lane_mask[[:space:]]*=|global_offer_lane_entry_slot_masks[[:space:]]*=" \
  "core.rs detected direct frontier cache mutation" \
  src/endpoint/kernel/core.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "route frontier owner check passed"
