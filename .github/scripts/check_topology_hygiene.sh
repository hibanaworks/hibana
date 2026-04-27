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
  -g '!tests/semantic_surface.rs'

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
  "MaterializedRouteBranch[[:space:]]*\\{[[:space:][:cntrl:]]*label:[[:space:]]*branch\\.label[[:space:][:cntrl:]]*,[[:space:][:cntrl:]]*binding_evidence:[[:space:]]*branch\\.binding_evidence[[:space:][:cntrl:]]*,[[:space:][:cntrl:]]*staged_payload:[[:space:]]*branch\\.staged_payload" \
  "tests must not copy RouteBranch preview resources into MaterializedRouteBranch" \
  src/endpoint/kernel tests

if ! rg -q "struct DecodeCommitPlan|build_decode_commit_plan|publish_decode_commit_plan" src/endpoint/kernel/decode.rs; then
  echo "topology hygiene violation: decode must carry branch, linger, loop, audit, cursor, and payload publish through DecodeCommitPlan" >&2
  FAILED=1
fi

for required in \
  "preflight_branch_preview_commit_plan" \
  "publish_branch_preview_commit_plan" \
  "RouteArmCommitProof" \
  "struct DecodeCommitTxn" \
  "DecodeCommitPlan<'txn, 'r>" \
  "route_arm_proofs: RouteCommitProofList<'txn>" \
  "with_decode_commit_txn" \
  "DecodeLingerCursorPlan" \
  "authorized_route_arm_for_decode" \
  "static_poll_route_arm_for_label" \
  "first_recv_target\\(scope" \
  "ok_or_else\\(decode_phase_invariant\\)" \
  "commit_route_arm_after_preflight" \
  "branch_commit_preflight_error_records_no_route_decisions" \
  "branch_commit_publish_is_infallible_after_preflight_and_preserves_refs"
do
  if ! rg -q "${required}" src/endpoint/kernel; then
    echo "topology hygiene violation: route branch commit must preflight fallible state before publishing side effects: ${required}" >&2
    FAILED=1
  fi
done

check_absent \
  "assert!\\([[:space:][:cntrl:]]*[^;]*set_route_arm|set_route_arm\\([^;]+\\)\\.is_ok\\(\\)" \
  "route branch publish must not assert over a fallible route-arm commit after side effects" \
  src/endpoint/kernel/route_frontier/offer.rs

check_absent \
  "ensure_current_route_arm_state|\\bset_route_arm\\b|preflight_set_route_arm" \
  "route state must be committed through RouteArmCommitProof, not repaired or set directly" \
  src/endpoint/kernel \
  -g '!**/core_offer_tests.rs'

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
  src/endpoint/kernel/runtime/route_state.rs

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
  'include_str!\("decode\.rs"\)|split\("fn |contains\("Self::static_poll_route_arm_for_label|contains\("\.ok_or_else\(decode_phase_invariant\)\?"' \
  "decode topology tests must execute runtime fixtures, not inspect source text" \
  src/endpoint/kernel/decode.rs src/endpoint/kernel/core_offer_tests.rs tests

check_absent \
  'include_str!\(' \
  "endpoint kernel tests must not inspect source text; use runtime proofs or shell hygiene gates" \
  src/endpoint/kernel tests

for required in \
  "dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit" \
  "static_linger_parent_route_commits_only_through_static_poll_descriptor"
do
  if ! rg -n "${required}" src/endpoint/kernel/core_offer_tests.rs tests >/dev/null; then
    echo "topology hygiene violation: missing runtime topology proof ${required}" >&2
    FAILED=1
  fi
done

check_absent \
  "commit_route_arm_after_preflight\\([^)]*scope|fn[[:space:]]+commit_route_arm_after_preflight\\([^)]*scope" \
  "route branch publish must consume a self-contained RouteArmCommitProof, not a separate scope argument" \
  src/endpoint/kernel

if rg -n -U "staged_payload[[:space:][:cntrl:]]*\\.take\\([[:space:][:cntrl:]]*\\)[[:space:][:cntrl:]]*\\.ok_or_else\\([^;]+;[[:space:][:cntrl:]]*branch\\.binding_evidence[[:space:]]*=[^;]+;[[:space:][:cntrl:]]*let payload[^;]+;[[:space:][:cntrl:]]*if self\\.cursor\\.try_advance_past_jumps_in_place\\(\\)" src/endpoint/kernel/decode.rs; then
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
for name in ("record_route_decision_for_scope_lanes", "skip_unselected_arm_lanes"):
    m = re.search(r"fn\s+" + name + r"[\s\S]*?\n    \}", source)
    if not m:
        print(f"topology hygiene violation: missing {name}", file=sys.stderr)
        sys.exit(1)
    body = m.group(0)
    forbidden = ["current_phase_lane_set"]
    if name == "skip_unselected_arm_lanes":
        forbidden.extend(["current_phase_contains_eff_index", "is_phase_complete"])
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
  src/endpoint/kernel/core.rs src/endpoint/kernel/decode.rs src/endpoint/kernel/recv.rs src/endpoint/kernel/runtime/route_state.rs src/endpoint/kernel/route_frontier

if rg -n -U "fn publish_route_branch_commit_plan\\([[:space:][:cntrl:]]*[^)]*\\)[[:space:][:cntrl:]]*->[[:space:][:cntrl:]]*RecvResult" src/endpoint/kernel/route_frontier/offer.rs; then
  echo "topology hygiene violation: branch commit publish phase must be infallible after preflight" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "topology hygiene check passed"
