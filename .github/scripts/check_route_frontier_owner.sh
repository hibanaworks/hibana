#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

fail() {
  echo "route frontier owner violation: $1" >&2
  FAILED=1
}

check_file() {
  local path="$1"
  [[ -s "${path}" ]] || fail "missing owner file ${path}"
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@" >/dev/null; then
    fail "${label}"
  fi
}

owner_manifest=".github/maintainability/route_frontier_owner_files.txt"
while IFS= read -r owner; do
  [[ -z "${owner}" || "${owner}" == \#* ]] && continue
  check_file "${owner}"
done < "${owner_manifest}"

check_absent \
  "transport_payload_len|transport_payload_lane|ProbeBinding \\{" \
  "offer frontier regressed to sentinel payload or ad-hoc probe state" \
  src/endpoint/kernel/core

check_absent \
  "ingress_evidence: \\[Option<|transport_payload: \\[Option<" \
  "offer rollback regressed to anonymous mini-vec ownership" \
  src/endpoint/kernel/offer.rs \
  src/endpoint/kernel/offer/state.rs

check_absent \
  "lane_route_arms:|root_frontier_state:|offer_entry_state:|scope_evidence:" \
  "core.rs reabsorbed split endpoint state owners" \
  src/endpoint/kernel/core.rs

check_absent \
  "lane_route_arms\\[[^]]+\\][[:space:]]*=|lane_linger_counts\\[[^]]+\\][[:space:]]*=|lane_offer_state\\[[^]]+\\][[:space:]]*=" \
  "core.rs reintroduced direct route-state table mutation" \
  src/endpoint/kernel/core.rs

check_absent \
  "offer_entry_state\\[[^]]+\\][[:space:]]*=|offer_entry_state\\.get_mut\\(|global_active_entries\\.(insert_entry|remove_entry)" \
  "core.rs reintroduced direct frontier table mutation" \
  src/endpoint/kernel/core.rs

check_absent \
  "root_frontier_state\\[[^]]+\\][[:space:]]*=|global_frontier_observed(_epoch|_key)?[[:space:]]*=|global_offer_lane_mask[[:space:]]*=|global_offer_lane_entry_slot_masks[[:space:]]*=" \
  "core.rs reintroduced direct frontier cache mutation" \
  src/endpoint/kernel/core.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "route frontier owner check passed"
