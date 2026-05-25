#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

semantic_common="tests/semantic_surface/common.rs"
for forbidden in \
  "pub(crate) fn read_dir_rs" \
  "pub(crate) fn read_rs_tree"
do
  if grep -Fq "${forbidden}" "${semantic_common}"; then
    echo "semantic reader hygiene violation: production readers must be explicitly named and test-filtered: ${forbidden}" >&2
    FAILED=1
  fi
done
for required in \
  "fn is_test_source" \
  "pub(crate) fn read_production_dir_rs" \
  "pub(crate) fn read_production_rs_tree" \
  "pub(crate) fn cursor_send_recv_tests_source"
do
  if ! grep -Fq "${required}" "${semantic_common}"; then
    echo "semantic reader hygiene violation: missing explicit source reader owner: ${required}" >&2
    FAILED=1
  fi
done

offer_resolve_source="$(cat src/endpoint/kernel/route_frontier/offer/resolve.rs src/endpoint/kernel/route_frontier/offer/passive.rs)"
for forbidden in \
  "resolved_hint_frame: Option<(u8, u8)>" \
  "poll_route_decision_authority: bool" \
  "route_token: &mut Option<RouteDecisionToken>" \
  "resolved_hint_frame: &mut Option<(u8, u8)>" \
  "poll_route_decision_authority: &mut bool" \
  "route_token: Option<RouteDecisionToken>" \
  "Poll<RecvResult<Option<ResolveTokenOutcome>>>" \
  "pending_action" \
  "yield_armed" \
  "ResolvePendingAction" \
  "RouteAuthoritySourceOutcome::Token {" \
  "RouteAuthoritySourceOutcome::EvidenceOnly" \
  "#[allow(clippy::too_many_arguments)]"
do
  if [[ "${offer_resolve_source}" == *"${forbidden}"* ]]; then
    echo "offer resolve decomposition violation: passive evidence collection must return a typed outcome instead of mutating resolver out-params: ${forbidden}" >&2
    FAILED=1
  fi
done
if rg -n "\\.(binding_evidence|transport_payload)" \
  src/endpoint/kernel/route_frontier/offer \
  --glob '!state.rs'
then
  echo "offer ingress owner violation: sibling modules must not access staged ingress fields directly" >&2
  FAILED=1
fi
if rg -n "poll_route_decision_authority" src/endpoint/kernel/route_frontier/offer; then
  echo "offer route authority violation: route-decision commit semantics must use typed evidence, not boolean authority flags" >&2
  FAILED=1
fi

if grep -Fq "pub(crate) enum CapError" src/rendezvous/error.rs \
  || grep -Fq "fn map_token_cap_error" src/rendezvous/core/cap_claim.rs
then
  echo "capability claim owner violation: rendezvous must use the canonical integration CapError without a mirror enum or mapper" >&2
  FAILED=1
fi

if grep -Fq "    lane_idx: usize," src/rendezvous/port/recv_frame.rs; then
  echo "received frame owner violation: ReceivedFrame must retain wire lane identity and derive usize indices only at use sites" >&2
  FAILED=1
fi

semantic_surface_sources="$(find tests/semantic_surface -type f -name '*.rs' -print0 | xargs -0 cat)"
for forbidden in \
  "segments: [[EffStruct; MAX_SEGMENT_EFFS]; MAX_SEGMENTS]" \
  "impl<Left, Right> BuildProgramSource for SeqSteps<Left, Right>" \
  "let has_ack =" \
  "let has_frame_hint =" \
  "if has_ack || has_frame_hint" \
  "pending_scope_frame_hint_on_lane(\\n                lane_idx" \
  "static_passive_dispatch_arm_from_exact_frame_label(\\n                                scope_id" \
  "terminal_clear_public_decode_state" \
  "state.take_rollback_items()" \
  "fn typed_header(&self)" \
  "expected_handle:" \
  "token.decode_handle()" \
  "claim_by_nonce(" \
  "OfferRunStage::CollectEvidence" \
  "OfferRunStage::ResolveToken" \
  "ProbeBinding {" \
  "payload_view(" \
  "fn poll_public_offer(" \
  ".split(\"pub trait Transport {\")" \
  "fn operational_deadline_ticks(&self) -> Option<u32> {\\n        None\\n    }" \
  "unreachable!(\"this fixture never exercises endpoint rollback\")" \
  "deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress" \
  "produce_non_wire_recv_evidence_requeues_staged_transport_payload" \
  "produce_wire_recv_frame_mismatch_is_terminal_without_requeue" \
  "direct_recv_requeues_transport_payload_when_binding_wins_after_poll_recv" \
  "enum SendPayloadPlan" \
  "fn prepare_send_payload_plan" \
  "fn stage_send_payload"
do
  if [[ "${semantic_surface_sources}" == *"${forbidden}"* ]]; then
    echo "semantic surface hygiene violation: behavior tests must not pin internal implementation shape: ${forbidden}" >&2
    FAILED=1
  fi
done

if (( FAILED != 0 )); then
  exit 1
fi

echo "semantic surface shape hygiene passed"
