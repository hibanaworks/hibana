#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  if rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "route frontier owner violation: ${label}" >&2
    FAILED=1
  fi
}

check_required() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  if ! rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "route frontier owner violation: ${label}" >&2
    FAILED=1
  fi
}

check_required_regex() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  if ! rg -n -U "${pattern}" "${path}" >/dev/null; then
    echo "route frontier owner violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent_outside_owner() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  local owner_a="$4"
  local owner_b="$5"
  if rg -n -F \
    --glob "!${owner_a}" \
    --glob "!${owner_b}" \
    --glob "!src/endpoint/kernel/core_offer_tests.rs" \
    "${pattern}" \
    "${path}" >/dev/null; then
    echo "route frontier owner violation: ${label}" >&2
    FAILED=1
  fi
}

OFFER=src/endpoint/kernel/route_frontier/offer.rs
OBS=src/endpoint/kernel/route_frontier/frontier_observation.rs
SELECT=src/endpoint/kernel/route_frontier/frontier_select.rs
SCOPE=src/endpoint/kernel/route_frontier/scope_evidence_logic.rs
REFRESH=src/endpoint/kernel/route_frontier/offer_refresh.rs

for required in \
  "struct RouteFrontierMachine<" \
  "fn record_scope_ack(" \
  "fn mark_scope_ready_arm(" \
  "fn mark_scope_ready_arm_from_frame_label(" \
  "fn mark_scope_ready_arm_from_binding_frame_label(" \
  "fn mark_static_passive_descendant_path_ready(" \
  "fn working_frontier_observation_cache(" \
  "fn ingest_binding_scope_evidence(" \
  "fn ingest_scope_evidence_for_offer(" \
  "fn recover_scope_evidence_conflict("; do
  check_required "${required}" "offer owner missing ${required}" "${OFFER}"
done

check_required_regex \
  "fn offer_entry_frame_label_meta\\([[:space:]\n]*endpoint: &CursorEndpoint" \
  "offer_entry_frame_label_meta must stay on RouteFrontierMachine" \
  "${OFFER}"

check_required_regex \
  "fn offer_refresh_mask\\([[:space:]\n]*endpoint: &CursorEndpoint" \
  "offer_refresh_mask must stay on RouteFrontierMachine" \
  "${OFFER}"

check_required_regex \
  "fn frontier_observation_offer_lane_entry_slot_masks\\([[:space:]\n]*endpoint: &CursorEndpoint" \
  "frontier_observation_offer_lane_entry_slot_masks must stay on RouteFrontierMachine" \
  "${OFFER}"

check_required_regex \
  "fn frontier_observation_key\\([[:space:]\n]*endpoint: &CursorEndpoint" \
  "frontier_observation_key must stay on RouteFrontierMachine" \
  "${OFFER}"

check_required_regex \
  "fn refresh_frontier_observation_cache\\([[:space:]\n]*endpoint: &'?[[:alnum:]_]*[[:space:]]*mut CursorEndpoint" \
  "refresh_frontier_observation_cache must stay on RouteFrontierMachine" \
  "${OFFER}"

for forbidden in \
  "fn offer_entry_frame_label_meta(&self," \
  "fn offer_refresh_mask(&self)" \
  "fn frontier_observation_lane_mask(&self," \
  "fn frontier_observation_offer_lane_entry_slot_masks(&self," \
  "fn frontier_observation_key(&self," \
  "fn refresh_frontier_observation_cache(&mut self,"; do
  check_absent "${forbidden}" "offer.rs must keep route-frontier helpers off CursorEndpoint ${forbidden}" "${OFFER}"
done

for required in \
  "fn scope_slot_for_route(" \
  "fn scope_evidence_generation_for_scope(" \
  "fn scope_ready_arm_mask(" \
  "fn static_passive_descendant_dispatch_arm_from_exact_frame_label("; do
  check_required "${required}" "scope-evidence helper owner missing ${required}" "${SCOPE}"
done

for forbidden in \
  "fn record_scope_ack(" \
  "fn mark_scope_ready_arm(" \
  "fn mark_scope_ready_arm_from_frame_label(" \
  "fn mark_scope_ready_arm_from_binding_frame_label(" \
  "fn mark_static_passive_descendant_path_ready(" \
  "fn ingest_binding_scope_evidence(" \
  "fn ingest_scope_evidence_for_offer(" \
  "fn recover_scope_evidence_conflict(" \
  "fn await_transport_payload_for_offer_lane(" \
  "fn await_static_passive_progress(" \
  "fn try_poll_route_decision_immediate(" \
  "fn try_poll_route_decision_for_offer("; do
  check_absent "${forbidden}" "scope_evidence_logic.rs regrew route-decision entrypoint ${forbidden}" "${SCOPE}"
done

for required in \
  "fn on_frontier_defer(" \
  "fn align_cursor_to_selected_scope(" \
  "fn try_poll_route_decision_immediate(" \
  "fn try_poll_route_decision_for_offer(" \
  "fn await_transport_payload_for_offer_lane(" \
  "fn await_static_passive_progress("; do
  check_required "${required}" "offer owner missing ${required}" "${OFFER}"
done

for forbidden in \
  "fn on_frontier_defer(" \
  "fn current_scope_selection_meta(" \
  "fn current_frontier_selection_state(" \
  "fn align_cursor_to_selected_scope(" \
  "fn frontier_observation_lane_mask(" \
  "fn frontier_observation_offer_lane_entry_slot_masks(" \
  "fn offer_entry_frame_label_meta(" \
  "fn ensure_global_frontier_scratch_initialized(" \
  "fn frontier_observation_cache(" \
  "fn store_frontier_observation(" \
  "fn cached_offer_entry_observed_state_for_rebuild(" \
  "fn refresh_frontier_observation_cache("; do
  check_absent "${forbidden}" "frontier_select.rs regrew delegated route-frontier entrypoint ${forbidden}" "${SELECT}"
done

for required in \
  "fn init_global_frontier_scratch_if_needed(" \
  "fn frontier_observation_cache_snapshot(" \
  "fn write_frontier_observation_snapshot(" \
  "fn reusable_cached_offer_entry_observed_state("; do
  check_required "${required}" "frontier-observation helper missing ${required}" "${OBS}"
done

for helper_call in \
  "init_global_frontier_scratch_if_needed(" \
  "frontier_observation_cache_snapshot(" \
  "write_frontier_observation_snapshot(" \
  "reusable_cached_offer_entry_observed_state("; do
  check_absent_outside_owner \
    "${helper_call}" \
    "frontier_observation helper leaked beyond offer.rs/frontier_observation.rs ${helper_call}" \
    "src/endpoint/kernel" \
    "${OFFER}" \
    "${OBS}"
done

for required in \
  "fn ensure_global_frontier_scratch_initialized(" \
  "fn frontier_observation_cache(" \
  "fn store_frontier_observation(" \
  "fn cached_offer_entry_observed_state_for_rebuild("; do
  check_required "${required}" "offer owner missing ${required}" "${OFFER}"
done

check_absent \
  "fn refresh_frontier_observation_cache(" \
  "frontier_observation.rs regrew refresh entrypoint" \
  "${OBS}"

for forbidden in \
  "fn ensure_global_frontier_scratch_initialized(" \
  "fn frontier_observation_cache(" \
  "fn store_frontier_observation(" \
  "fn cached_offer_entry_observed_state_for_rebuild("; do
  check_absent "${forbidden}" "frontier_observation.rs regrew route-frontier owner ${forbidden}" "${OBS}"
done

check_absent \
  "fn frontier_observation_key(" \
  "frontier_observation.rs regrew delegated observation-key entrypoint" \
  "${OBS}"

check_absent \
  "fn working_frontier_observation_cache(" \
  "frontier_observation.rs regrew delegated working-cache entrypoint" \
  "${OBS}"

check_absent \
  "fn frontier_observation_lane_mask(" \
  "frontier_observation.rs regrew delegated observation-mask entrypoint" \
  "${OBS}"

check_absent \
  "fn frontier_observation_offer_lane_entry_slot_masks(" \
  "frontier_observation.rs regrew delegated observation-slot entrypoint" \
  "${OBS}"

for required in \
  "fn root_frontier_active_mask(" \
  "fn active_frontier_entries(" \
  "fn compute_offer_entry_static_summary("; do
  check_required "${required}" "offer-refresh owner missing ${required}" "${REFRESH}"
done

check_absent \
  "fn offer_refresh_mask(" \
  "offer_refresh.rs regrew delegated refresh-mask entrypoint" \
  "${REFRESH}"

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "route frontier owner check passed"
