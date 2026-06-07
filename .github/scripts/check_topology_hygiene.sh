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
    echo "topology hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent \
  "\\bsplice\\b" \
  "legacy splice topology vocabulary in core source or public README" \
  src README.md

check_absent \
  "validate_splice_generation|cached_splice|splice_table|splice_graph|splice operands|splice control" \
  "legacy splice topology vocabulary" \
  src tests \
  -g '!tests/ui.rs' \
  -g '!tests/docs_surface.rs' \
  -g '!tests/ui/core_splice_kind_reintroduction.rs' \
  -g '!tests/ui/core_splice_kind_reintroduction.stderr'

check_absent \
  "topology_operands_from_route_input|prepare_topology_operands_from_policy|validate_topology_operands_from_policy" \
  "topology operands decoded from policy route input" \
  src tests

check_absent \
  "topology_flags|FENCES_PRESENT" \
  "topology handle flags reintroduced instead of reserved-zero descriptors" \
  src tests

check_absent \
  "topology_operands_from_handle|descriptor\\.handle\\(|struct[[:space:]]+TopologyDescriptor[[:space:]]*\\{[[:space:][:cntrl:]]*handle:" \
  "TopologyDescriptor regressed into a runtime handle wrapper instead of typed topology facts" \
  src tests

check_absent \
  "pub\\(super\\)[[:space:]]+fn[[:space:]]+topology_commit\\(|\\.topology\\.topology_commit\\(" \
  "test-only topology commit owner bypasses cluster-owned production commit path" \
  src/rendezvous src/control tests \
  -g '!tests/semantic_surface.rs' \
  -g '!tests/semantic_surface/**'

check_absent \
  "POLICY_MODE_ENFORCE_TAG|PolicyVerdict::Proceed" \
  "core audit conflates no-engine with enforce/proceed" \
  src tests

check_absent \
  "typed_branch_from_materialized|[^[:alnum:]_]RouteBranch[[:space:]]*\\{[[:space:][:cntrl:]]*label:[[:space:]]*branch\\.label" \
  "materialized route branch must not be cloned back into an owning RouteBranch" \
  src/endpoint/kernel tests

check_absent \
  "impl[[:space:][:cntrl:]]*<[^>]*>[[:space:][:cntrl:]]*From<MaterializedRouteBranch[^>]*>[[:space:][:cntrl:]]*for[[:space:][:cntrl:]]*RouteBranch|impl[[:space:][:cntrl:]]*<[^>]*>[[:space:][:cntrl:]]*Into<RouteBranch[^>]*>[[:space:][:cntrl:]]*for[[:space:][:cntrl:]]*MaterializedRouteBranch|fn[[:alnum:]_]*materialized[[:alnum:]_]*route[[:alnum:]_]*branch[[:alnum:]_]*RouteBranch" \
  "MaterializedRouteBranch must not have a re-owning conversion back into RouteBranch" \
  src/endpoint/kernel tests

check_absent \
  "MaterializedRouteBranch[[:space:]]*\\{[[:space:][:cntrl:]]*label:[[:space:]]*branch\\.label[[:space:][:cntrl:]]*,[[:space:][:cntrl:]]*ingress_evidence:[[:space:]]*branch\\.ingress_evidence[[:space:][:cntrl:]]*,[[:space:][:cntrl:]]*staged_payload:[[:space:]]*branch\\.staged_payload" \
  "tests must not copy RouteBranch preview resources into MaterializedRouteBranch" \
  src/endpoint/kernel tests

if ! rg -q "struct DecodeCommitPlan|build_decode_commit_plan|publish_decode_commit_plan" src/endpoint/kernel/decode.rs; then
  echo "topology hygiene violation: decode must carry branch, linger, loop, audit, cursor, and payload publish through DecodeCommitPlan" >&2
  FAILED=1
fi

for required in \
  "preflight_branch_preview_commit_plan" \
  "publish_branch_preview_commit_plan" \
  "struct DecodeCommitTxn" \
  "DecodeCommitPlan<'r>" \
  "with_selected_route_rows" \
  "SelectedRouteCommitRowsRef" \
  "CommitDelta::route_rows" \
  "LoopCommitRow" \
  "PreparedDecodePublishPlan" \
  "with_decode_commit_txn" \
  "DecodeLingerCursorPlan" \
  "SelectedRouteCommitRow" \
  "prepare_commit_delta" \
  "event_enabled\\(" \
  "prepare_selected_route_commit_row_from_parts" \
  "commit_prepared_delta\\(delta\\)"
do
  if ! rg -q "${required}" src/endpoint/kernel; then
    echo "topology hygiene violation: route branch commit must preflight fallible state before publishing side effects: ${required}" >&2
    FAILED=1
  fi
done

check_absent \
  "assert!\\([[:space:][:cntrl:]]*[^;]*set_route_arm|set_route_arm\\([^;]+\\)\\.is_ok\\(\\)" \
  "route branch publish must not assert over a fallible route-arm commit after side effects" \
  src/endpoint/kernel/offer.rs

check_absent \
  "ensure_current_route_arm_state|\\bset_route_arm\\b|preflight_set_route_arm" \
  "route state must be committed through SelectedRouteCommitRow, not repaired or set directly" \
  src/endpoint/kernel

check_absent \
  "RouteArmCommitProof|RouteCommitProofList|route_arm_proofs|commit_route_arm_after_preflight" \
  "old route-arm proof topology path must not be reintroduced" \
  src/endpoint/kernel

check_absent \
  "publish_route_arm_commit\\(" \
  "route-arm commit must be split into explicit proof build and infallible publish; no combined fallible helper" \
  src/endpoint/kernel \
  -g '!**/*tests.rs'

check_absent \
  "MAX_ROUTE_ARM_PROOF_LIST|MAX_DECODE_LINGER_ROUTE_ARM_PROOFS|ROUTE_ARM_PROOF_STORAGE|DECODE_ROUTE_ARM_PROOF_STORAGE|MAX_SEGMENTS[[:space:]]*\\*[[:space:]]*2" \
  "route-arm proof storage must derive from compiled route-scope count, not lowering segment count or hidden magic caps" \
  src/endpoint/kernel

check_absent \
  "struct RouteCommitProofList[^{]*\\{[^}]*\\*mut|fn begin\\(self, required" \
  "route commit proof workspace must be an affine borrowed slice, not a raw pointer writer" \
  src/endpoint/kernel/decision_state.rs

check_absent \
  "transmute::<[^>]*RouteCommitProofList|RouteCommitProofList<'r>|core::mem::transmute" \
  "route commit proof lease lifetime must not be widened or hidden" \
  src/endpoint/kernel

check_absent \
  "publish_linger_decode_cursor_repair|cursor_repair|\\brepair\\b" \
  "decode cursor movement must be a preflighted DecodeCommitPlan field, not a post-commit repair path" \
  src/endpoint/kernel/decode.rs

check_absent \
  "first_recv_target_evidence" \
  "route arm authority must not be derived from label/demux evidence" \
  src

check_absent \
  "endpoint:[[:space:]]*\\*mut[[:space:]]+CursorEndpoint|fn endpoint_mut\\(" \
  "decode commit transaction must field-split Endpoint, not recover &mut Endpoint from raw pointer" \
  src/endpoint/kernel/decode.rs

check_absent \
  "publish_len\\(|proof_at\\(|proof_count|route_arm_proof_count|branch_route_proof_count" \
  "route commit proofs must be consumed as RouteCommitProofList, not hidden in workspace by count" \
  src/endpoint/kernel

check_absent \
  'include_str!\("decode\.rs"\)|split\("fn |contains\("Self::static_poll_route_arm_for_lane_frame_label|contains\("\.ok_or_else\(decode_phase_invariant\)\?"' \
  "decode topology tests must execute runtime fixtures, not inspect source text" \
  src/endpoint/kernel/decode.rs tests \
  -g '!tests/semantic_surface.rs' \
  -g '!tests/semantic_surface/**'

check_absent \
  'include_str!\(' \
  "endpoint kernel tests must not inspect source text; use runtime proofs or shell hygiene gates" \
  src/endpoint/kernel tests \
  -g '!tests/semantic_surface.rs' \
  -g '!tests/semantic_surface/**'

for required in \
  "route_selected_left_keeps_entire_nested_parallel_path_live" \
  "alternating_route_parallel_join_uses_only_selected_arms" \
  "unselected_route_arm_parallel_events_are_dead_and_not_join_obligations" \
  "unselected_route_arm_parallel_events_do_not_block_parallel_join" \
  "outer_left_selection_kills_nested_right_route_and_parallel_body"
do
  if ! rg -n "${required}" tests >/dev/null; then
    echo "topology hygiene violation: missing runtime topology proof ${required}" >&2
    FAILED=1
  fi
done

check_absent \
  "commit_route_arm_after_preflight\\([^)]*scope|fn[[:space:]]+commit_route_arm_after_preflight\\([^)]*scope" \
  "route branch publish must consume selected-route commit rows, not a separate scope argument" \
  src/endpoint/kernel

if rg -n -U "staged_payload[[:space:][:cntrl:]]*\\.take\\([[:space:][:cntrl:]]*\\)[[:space:][:cntrl:]]*\\.ok_or_else\\([^;]+;[[:space:][:cntrl:]]*branch\\.ingress_evidence[[:space:]]*=[^;]+;[[:space:][:cntrl:]]*let payload[^;]+;[[:space:][:cntrl:]]*if self\\.cursor\\.try_advance_past_jumps_in_place\\(\\)" src/endpoint/kernel/decode.rs; then
  echo "topology hygiene violation: decode must not consume preview payload before jump advance can fail" >&2
  FAILED=1
fi

if rg -n -U "fn prepare_decode_transport_wait[[:space:][:cntrl:]]*\\([^}]+(loop_table\\(\\)\\.acknowledge|ack_loop_decision)" src/endpoint/kernel/decode.rs; then
  echo "topology hygiene violation: decode prepare phase must not publish loop acknowledgements before payload validation" >&2
  FAILED=1
fi

check_absent \
  "try_advance_past_jumps_in_place\\(|try_follow_jumps_in_place\\(" \
  "decode must preflight jump traversal and publish cursor movement infallibly" \
  src/endpoint/kernel/decode.rs

check_absent \
  "apply_branch_recv_policy|preflight_branch_recv_policy|publish_branch_recv_audit" \
  "decode audit emission must be separated into preflight plan and post-commit publish" \
  src/endpoint/kernel/decode.rs

check_absent \
  "BranchKind::LocalControl[[:space:][:cntrl:]]*[=>{][[:space:][:cntrl:][:print:]]*commit_branch_preview_view|BranchKind::EmptyArmTerminal[[:space:][:cntrl:]]*[=>{][[:space:][:cntrl:][:print:]]*commit_branch_preview_view|BranchKind::ArmSendHint[[:space:][:cntrl:]]*[=>{][[:space:][:cntrl:][:print:]]*commit_branch_preview_view" \
  "decode branch commit must publish through DecodeCommitPlan for every branch kind" \
  src/endpoint/kernel/decode.rs

check_absent \
  "arm1_lane_word_start\\(\\).*route_arm0_lane_words_by_dense_route|let start = route\\.arm1_lane_word_start\\(\\)|arm1_lane_word_start" \
  "route arm lane row start must not be named as arm1-only when shared by both arm tables" \
  src/global/compiled src/global/typestate

python3 - <<'PY'
import pathlib
import sys

source = pathlib.Path("src/endpoint/kernel/decode.rs").read_text()
if "build_linger_route_arm_commit_plan(" in source or "publish_linger_route_arm_commit_plan(" in source:
    print(
        "topology hygiene violation: decode linger route-arm proofs must be part of DecodeCommitPlan",
        file=sys.stderr,
    )
    sys.exit(1)
if "publish_endpoint_rx_audit(plan.audit);" in source:
    tail = source.split("publish_endpoint_rx_audit(plan.audit);", 1)[1]
    if "ok_or_else" in tail.split("Ok(payload)", 1)[0]:
        print(
            "topology hygiene violation: decode has fallible payload take after audit publish",
            file=sys.stderr,
        )
        sys.exit(1)
PY

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/endpoint/kernel/core.rs").read_text()
for path in sorted(pathlib.Path("src/endpoint/kernel/core").rglob("*.rs")):
    source += "\n" + path.read_text()
if "skip_unselected_arm_lanes" in source:
    print(
        "topology hygiene violation: unselected route arms must not be skipped by endpoint topology walkers",
        file=sys.stderr,
    )
    sys.exit(1)
for name in ("record_route_decision_for_scope_lanes",):
    m = re.search(r"fn\s+" + name + r"[\s\S]*?\n    \}", source)
    if not m:
        print(f"topology hygiene violation: missing {name}", file=sys.stderr)
        sys.exit(1)
    body = m.group(0)
    forbidden = ["current_phase_lane_set"]
    for token in forbidden:
        if token not in body:
            continue
        print(
            f"topology hygiene violation: {name} must not use phase-local authority: {token}",
            file=sys.stderr,
        )
        sys.exit(1)
PY

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/endpoint/kernel/decode.rs").read_text()
if re.search(r"fn\s+publish_decode_commit_plan\s*\([^{}]*?\)\s*->\s*RecvResult", source, re.S):
    print(
        "topology hygiene violation: decode publish phase must be infallible after DecodeCommitPlan preflight",
        file=sys.stderr,
    )
    sys.exit(1)
PY

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/endpoint/kernel/decode.rs").read_text()
if re.search(r"publish_branch_recv_audit\(audit_plan\);[\s\S]{0,240}\?", source):
    print(
        "topology hygiene violation: decode audit publish must not precede fallible work",
        file=sys.stderr,
    )
    sys.exit(1)
PY

check_absent \
  "while[[:space:]]+lane_idx[[:space:]]*<[[:space:]]*(logical_lane_count|lane_limit|self\\.cursor\\.logical_lane_count\\(\\)|self\\.lane_offer_states\\.lane_slot_count)|for[[:space:]]+lane_idx[[:space:]]+in[[:space:]]+0\\.\\.(logical_lane_count|lane_limit)" \
  "endpoint kernel hot path must walk active lane sets, not scan every logical lane" \
  src/endpoint/kernel/core.rs src/endpoint/kernel/decode.rs src/endpoint/kernel/recv.rs src/endpoint/kernel/decision_state.rs src/endpoint/kernel/core

if rg -n -U "fn publish_route_branch_commit_plan\\([[:space:][:cntrl:]]*[^)]*\\)[[:space:][:cntrl:]]*->[[:space:][:cntrl:]]*RecvResult" src/endpoint/kernel/offer.rs src/endpoint/kernel/offer; then
  echo "topology hygiene violation: branch commit publish phase must be infallible after preflight" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "topology hygiene check passed"
