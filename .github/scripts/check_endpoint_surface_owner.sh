#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

POLL_READY_BLOCK="$(
  awk '
    /fn poll_arm_from_ready_mask\(/ { in_block=1 }
    in_block {
      print
      if ($0 ~ /^    }$/) { exit }
    }
  ' src/endpoint/kernel/core/scope_evidence_logic.rs
)"
if [[ -z "${POLL_READY_BLOCK}" ]]; then
  echo "poll_arm_from_ready_mask block not found" >&2
  FAILED=1
else
  check_pipe_absent "\\bscope_ready_arm_mask\\(" \
    "poll_arm_from_ready_mask must not read demux/materialization-ready mask" \
    "${POLL_READY_BLOCK}"
fi

ROUTE_TOKEN_BLOCK="$(
  awk '
    /enum RouteArmToken/ { in_block=1; next }
    in_block {
      if ($0 ~ /^}/) { exit }
      print
    }
  ' src/endpoint/kernel/authority.rs
)"
check_absent "RouteAuthoritySource" \
  "RouteAuthoritySource enum must stay forbidden" \
  src/endpoint/kernel/authority.rs
if [[ -z "${ROUTE_TOKEN_BLOCK}" ]]; then
  echo "RouteArmToken enum block not found" >&2
  FAILED=1
else
  ROUTE_TOKEN_VARIANTS="$(
    printf '%s\n' "${ROUTE_TOKEN_BLOCK}" \
      | awk '
          /^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*(\([^)]*\))?,?[[:space:]]*$/ {
            value=$0
            sub(/^[[:space:]]+/, "", value)
            sub(/\(.*/, "", value)
            sub(/[[:space:]]*,?[[:space:]]*$/, "", value)
            print value
          }
        '
  )"
  if [[ -z "${ROUTE_TOKEN_VARIANTS}" ]]; then
    echo "RouteArmToken variants not found" >&2
    FAILED=1
  else
    BAD_ROUTE_TOKEN_VARIANTS="$(
      printf '%s\n' "${ROUTE_TOKEN_VARIANTS}" \
        | awk '$0 !~ /^(Ack|Resolver|Poll)$/ { print NR ":" $0 }'
    )"
    if [[ -n "${BAD_ROUTE_TOKEN_VARIANTS}" ]]; then
      echo "${BAD_ROUTE_TOKEN_VARIANTS}" >&2
      echo "RouteArmToken domain violation (expected Ack|Resolver|Poll only)" >&2
      FAILED=1
    fi
  fi
fi

for owner in \
  src/endpoint/kernel/offer/facts.rs \
  src/endpoint/kernel/offer/frontier_types.rs \
  src/endpoint/kernel/offer/ingress.rs \
  src/endpoint/kernel/offer/ingress_types.rs \
  src/endpoint/kernel/offer/materialization.rs \
  src/endpoint/kernel/offer/resolve_types.rs \
  src/endpoint/kernel/offer/state.rs \
  src/endpoint/kernel/offer/commit_types.rs
do
  if [[ ! -s "${owner}" ]]; then
    echo "offer frontier owner module missing: ${owner}" >&2
    FAILED=1
  fi
done

check_absent "transport_payload_len|transport_payload_lane|ProbeBinding \\{" \
  "offer frontier regressed to implicit payload cache or probe-owned state" \
  src/endpoint/kernel/core

check_absent "ingress_evidence: \\[Option<|transport_payload: \\[Option<" \
  "offer restore regressed to anonymous mini-vec ownership" \
  src/endpoint/kernel/offer.rs \
  src/endpoint/kernel/offer/state.rs

check_absent "lane_route_arms:|root_frontier_state:|offer_entry_state:|scope_evidence:" \
  "core.rs reabsorbed split endpoint state owners" \
  src/endpoint/kernel/core.rs

if [[ ! -s "src/endpoint/kernel/public_ops.rs" ]]; then
  echo "public endpoint operation owner module missing" >&2
  FAILED=1
fi

for forbidden in \
  "fn restore_materialized_route_branch(" \
  "fn reset_public_offer_state(" \
  "fn terminal_clear_public_offer_state(" \
  "fn reset_public_send_state(" \
  "fn poll_public_offer(" \
  "fn poll_public_recv(" \
  "fn poll_public_decode(" \
  "fn poll_public_send("
do
  check_absent_literal "${forbidden}" \
    "core.rs reabsorbed public endpoint operation lifecycle: ${forbidden}" \
    src/endpoint/kernel/core.rs
done

check_absent "payload_view\\(" \
  "received transport frame payload view detected instead of intent-specific frame operations" \
  src/endpoint src/rendezvous/port.rs

check_absent "lane_route_arms\\[[^]]+\\][[:space:]]*=|lane_reentry_counts\\[[^]]+\\][[:space:]]*=|lane_offer_state\\[[^]]+\\][[:space:]]*=" \
  "core.rs detected direct route-state table mutation" \
  src/endpoint/kernel/core.rs

check_absent "offer_entry_state\\[[^]]+\\][[:space:]]*=|offer_entry_state\\.get_mut\\(|global_active_entries\\.(insert_entry|remove_entry)" \
  "core.rs detected direct frontier table mutation" \
  src/endpoint/kernel/core.rs

check_absent "root_frontier_state\\[[^]]+\\][[:space:]]*=|global_frontier_observed(_generation|_key)?[[:space:]]*=|global_offer_lane_mask[[:space:]]*=|global_offer_lane_entry_slot_masks[[:space:]]*=" \
  "core.rs detected direct frontier cache mutation" \
  src/endpoint/kernel/core.rs

for forbidden in \
  "fn record_scope_ack(" \
  "fn ingest_scope_evidence_for_offer(" \
  "fn on_frontier_defer(" \
  "fn align_cursor_to_selected_scope(" \
  "fn frontier_observation_key(" \
  "fn refresh_frontier_observation_cache(" \
  "fn compose_frontier_observed_entries(" \
  "fn offer_refresh_mask(" \
  "fn next_frontier_observation_generation(" \
  "fn offer_entry_candidate_from_observation(" \
  "fn refresh_offer_entry_state(" \
  "fn sync_lane_offer_state(" \
  "fn refresh_lane_offer_state("
do
  check_absent_literal "${forbidden}" \
    "core.rs reabsorbed split endpoint logic owners: ${forbidden}" \
    src/endpoint/kernel/core.rs
done

check_required_regex "mod evidence_store;|mod frontier_state;|mod route_state;" \
  "kernel mod split owner deletion" \
  src/endpoint/kernel/mod.rs

for required in \
  "src/endpoint/kernel/evidence_store.rs:pub\\(super\\) struct ScopeEvidenceTable" \
  "src/endpoint/kernel/frontier_state.rs:pub\\(super\\) struct FrontierState" \
  "src/endpoint/kernel/decision_state.rs:pub\\(super\\) struct RouteState"
do
  path="${required%%:*}"
  pattern="${required#*:}"
  check_required_regex "${pattern}" \
    "split endpoint owner modules missing: ${required}" \
    "${path}"
done

for required in \
  'src/endpoint/kernel/mod.rs:mod offer;' \
  'src/endpoint/kernel/offer.rs:mod select;' \
  'src/endpoint/kernel/offer.rs:mod select_alignment;' \
  'src/endpoint/kernel/offer.rs:mod ingress;' \
  'src/endpoint/kernel/offer.rs:mod profile;' \
  'src/endpoint/kernel/offer.rs:mod first_recv_dispatch;' \
  'src/endpoint/kernel/offer.rs:mod resolve;' \
  'src/endpoint/kernel/offer.rs:mod materialization;'
do
  path="${required%%:*}"
  pattern="${required#*:}"
  check_required "${pattern}" \
    "split endpoint logic owner missing: ${required}" \
    "${path}"
done
check_absent \
  "fn record_scope_ack\\(|fn on_frontier_defer\\(|fn frontier_observation_key\\(" \
  "split endpoint logic owner violation: selection/frontier helper implementations must stay in offer owner shards, not the root offer facade" \
  src/endpoint/kernel/offer.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "endpoint surface owner check passed"
