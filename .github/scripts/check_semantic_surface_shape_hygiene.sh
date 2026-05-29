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
if rg -n '^#!\[allow\((private_interfaces|unused_imports|.*private_interfaces.*|.*unused_imports.*)\)\]' \
  src --glob '*.rs'
then
  echo "warning-suppression hygiene violation: production modules must fix private-interface and unused-import warnings instead of hiding them" >&2
  FAILED=1
fi
if rg -n 'ProjectionMessageSpec|ProjectionTypeFingerprint|VisitProjectionMessages|visit_projection_messages|visit_message' \
  src --glob '*.rs'
then
  echo "projection metadata violation: public/runtime projection metadata must be Pico-compatible numeric facts only" >&2
  FAILED=1
fi
if ! python3 - <<'PY'
from pathlib import Path
import re
import sys

root = Path("src")
violations = []

def test_only_path(path: Path) -> bool:
    parts = set(path.parts)
    return (
        "test_support" in parts
        or path.name == "tests.rs"
        or path.name.endswith("_tests.rs")
        or path.name.endswith("_test.rs")
        or "tests" in parts
    )

for path in sorted(root.rglob("*.rs")):
    if test_only_path(path):
        continue
    depth = 0
    pending_test_cfg = False
    test_module_depths = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("#[cfg(") and "test" in stripped:
            pending_test_cfg = True
        in_test_module = any(depth >= marker for marker in test_module_depths)
        if not in_test_module and re.match(r"^\s*use\s+super::\*;", line):
            violations.append(f"{path}:{lineno}: {line.strip()}")
        opens = line.count("{")
        closes = line.count("}")
        if pending_test_cfg and re.search(r"\bmod\s+[A-Za-z0-9_]+\s*\{", line):
            test_module_depths.append(depth + opens)
            pending_test_cfg = False
        elif stripped and not stripped.startswith("#["):
            pending_test_cfg = False
        depth += opens - closes
        while test_module_depths and depth < test_module_depths[-1]:
            test_module_depths.pop()

if violations:
    print("\n".join(violations), file=sys.stderr)
    sys.exit(1)
PY
then
  echo "production import hygiene violation: production modules must name parent imports explicitly instead of inheriting the parent namespace" >&2
  FAILED=1
fi

if ! python3 - <<'PY'
from pathlib import Path
import sys

root = Path("src")
violations = []

def test_only_path(path: Path) -> bool:
    parts = set(path.parts)
    return (
        "test_support" in parts
        or path.name == "tests.rs"
        or path.name.endswith("_tests.rs")
        or path.name.endswith("_test.rs")
        or "tests" in parts
    )

for path in sorted(root.rglob("*.rs")):
    if test_only_path(path):
        continue
    lines = path.read_text(encoding="utf-8").splitlines()
    for idx, line in enumerate(lines):
        if not line.strip().startswith("#[path"):
            continue
        lookback = "\n".join(lines[max(0, idx - 4):idx])
        if (
            "#[cfg(test)]" in lookback
            or "cfg(all(test, hibana_repo_tests))" in lookback
            or "cfg_attr(test" in lookback
        ):
            continue
        violations.append(f"{path}:{idx + 1}: {line.strip()}")

if violations:
    print("\n".join(violations), file=sys.stderr)
    sys.exit(1)
PY
then
  echo "production module graph violation: non-test modules must use standard module paths instead of #[path] shims" >&2
  FAILED=1
fi

offer_resolve_source="$(cat \
  src/endpoint/kernel/offer/resolve.rs \
  src/endpoint/kernel/offer/resolve_materialization.rs \
  src/endpoint/kernel/offer/passive.rs)"
if rg -n '^use super::.*\*;' \
  src/endpoint/kernel/offer/resolve.rs \
  src/endpoint/kernel/offer/resolve_materialization.rs
then
  echo "offer resolve decomposition violation: route authority owners must name imported dependencies explicitly instead of using wildcard imports to line-golf near owner budgets" >&2
  FAILED=1
fi
if rg -n '^use super::.*\*;' \
  src/endpoint/kernel/core/route_commit_helpers.rs \
  src/global/compiled/images/image/role_descriptor_ref/route_scope/dispatch.rs
then
  echo "module decomposition violation: extracted owner shards must name imported dependencies explicitly instead of inheriting the parent namespace" >&2
  FAILED=1
fi
if rg -n 'first_recv_dispatch_(arm_mask|lane_mask)\(|route_scope_first_recv_dispatch_(frame_label_mask|arm_mask|lane_mask|arm_frame_label_mask)\(' \
  src/endpoint/kernel/core/frontier_select.rs \
  src/endpoint/kernel/core/frontier_helpers.rs \
  src/global/compiled/images/image/role_descriptor_ref/route_scope/dispatch.rs \
  src/global/typestate/cursor/scope_route.rs
then
  echo "first-recv dispatch violation: hot paths must derive dispatch masks from one table pass instead of rescanning resident route arms" >&2
  FAILED=1
fi
if rg -n "frame_label_mask" src/endpoint/kernel/offer/first_recv_dispatch.rs; then
  echo "first-recv dispatch violation: offer dispatch cache must not store unused frame-label masks on Pico resident state" >&2
  FAILED=1
fi
if grep -Fq "first_recv_dispatch: [(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH]" \
  src/endpoint/kernel/offer/frontier_types.rs
then
  echo "first-recv dispatch violation: offer materialization must not store positional dispatch tuples in its hot-path cache" >&2
  FAILED=1
fi
if ! grep -Fq "pub(crate) struct FirstRecvDispatchSpec" src/global/typestate/facts.rs \
  || ! grep -Fq "FirstRecvDispatchSpec::new(" src/global/compiled/images/image/role_descriptor_ref/route_scope/dispatch.rs \
  || rg -n '\[\(u8, u8, u8, StateIndex\); MAX_FIRST_RECV_DISPATCH\]|Option<\(u8, u8, u8, StateIndex\)>|from_tuple' src tests --glob '!tests/semantic_surface/**'
then
  echo "first-recv dispatch violation: compiled dispatch must cross layers as typed FirstRecvDispatchSpec values, never positional tuples" >&2
  FAILED=1
fi
for forbidden in \
  "resolved_hint_frame: Option<(u8, u8)>" \
  "poll_route_decision_authority: bool" \
  "route_token: &mut Option<RouteDecisionToken>" \
  "resolved_hint_frame: &mut Option<(u8, u8)>" \
  "poll_route_decision_authority: &mut bool" \
  "route_token: Option<RouteDecisionToken>" \
  "let mut route_token = self.endpoint.peek_scope_ack" \
  "route_token.is_none()" \
  "authority: &mut RouteAuthorityResolution" \
  "let mut authority = match self.collect_route_authority" \
  "authority.route_token =" \
  "authority.commit_evidence =" \
  "MaterializedRouteAuthority" \
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
  src/endpoint/kernel/offer \
  --glob '!state.rs'
then
  echo "offer ingress owner violation: sibling modules must not access staged ingress fields directly" >&2
  FAILED=1
fi
if rg -n "poll_route_decision_authority" src/endpoint/kernel/offer; then
  echo "offer route authority violation: route-decision commit semantics must use typed evidence, not boolean authority flags" >&2
  FAILED=1
fi
if [[ -e src/endpoint/kernel/offer/endpoint_bridge.rs \
  || -e src/endpoint/kernel/offer/machine.rs ]]
then
  echo "offer frontier owner violation: route frontier must not hide CursorEndpoint ownership behind identity bridge/machine wrappers" >&2
  FAILED=1
fi
if rg -n "RouteFrontierMachine|NoParkedTransportForTest" \
  src/endpoint/kernel/offer \
  src/endpoint/kernel/test_support/core_offer_tests
then
  echo "offer frontier owner violation: tests and production code must use the real CursorEndpoint owner, not identity machines or test-only drain modes" >&2
  FAILED=1
fi
if rg -n "run_stage: Option|frontier_visited: Option" \
  src/endpoint/kernel/offer/state.rs
then
  echo "offer state owner violation: offer execution phase must be a total enum, not nullable phase fields" >&2
  FAILED=1
fi
offer_frontier_facts_block="$(
  sed -n '/struct OfferFrontierFacts {/,/^}/p' src/endpoint/kernel/offer/ingress.rs
)"
for forbidden_fact_field in \
  "is_route_controller" \
  "is_dynamic_route_scope" \
  "suppress_scope_frame_hint"
do
  if [[ "${offer_frontier_facts_block}" == *"${forbidden_fact_field}"* ]]; then
    echo "offer route profile violation: OfferFrontierFacts must not leak route-shape booleans: ${forbidden_fact_field}" >&2
    FAILED=1
  fi
done
if rg -n "is_route_controller\\(|route_scope_controller_policy\\(" \
  src/endpoint/kernel/offer/commit.rs
then
  echo "offer route profile violation: branch commit must use carried profile metadata instead of re-querying cursor route shape" >&2
  FAILED=1
fi
if grep -Fq "let pending_recv = lane_port::PendingRecv::new();" \
  src/endpoint/kernel/offer.rs
then
  echo "offer scope evidence violation: production offer code must not fake an empty PendingRecv to force hint draining" >&2
  FAILED=1
fi
if grep -Fq "OfferScopeEvidenceDrain" src/endpoint/kernel/offer.rs; then
  echo "offer scope evidence violation: hint draining must be represented as a concrete authority, not a single-variant future enum" >&2
  FAILED=1
fi
if grep -Fq "from_controller_dynamic(is_controller: bool, is_dynamic: bool)" \
  src/endpoint/kernel/offer/profile.rs
then
  echo "offer route profile violation: route profile must be built from typed route evidence, not raw boolean pairs" >&2
  FAILED=1
fi
python3 - <<'PY' || FAILED=1
from pathlib import Path
facts_rs = Path("src/endpoint/kernel/offer/facts.rs").read_text()
facts = (
    facts_rs
    + Path("src/endpoint/kernel/offer/facts/evidence.rs").read_text()
    + Path("src/endpoint/kernel/offer/facts/planner.rs").read_text()
    + Path("src/endpoint/kernel/offer/profile.rs").read_text()
    + Path("src/endpoint/kernel/offer/profile/evidence.rs").read_text()
)
start = facts_rs.find("pub(super) fn prepare_frontier_facts(")
end = facts_rs.find("\n    fn ", start)
if start < 0:
    raise SystemExit("offer route profile violation: prepare_frontier_facts must delegate to typed ingress evidence")
if end < 0:
    end = len(facts_rs)
body = facts_rs[start:end]
if "self.offer_ingress_evidence(" not in body:
    raise SystemExit("offer route profile violation: prepare_frontier_facts must publish typed ingress evidence instead of assembling booleans")
for token in [
    "OfferRouteShape",
    "OfferControllerArmEntry::from_present",
    "OfferControllerCursorArm::from_present",
    "OfferPassiveRecvEvidence::from_has_recv",
    "OfferPassiveAckEvidence::from_materializable",
    "OfferMaterializationReadiness::from_pending",
    "unwrap_or(false)",
]:
    if token in body:
        raise SystemExit(
            f"offer route profile violation: prepare_frontier_facts must not assemble boolean evidence directly: {token}"
        )
for token in [
    "struct OfferIngressEvidence",
    "fn offer_ingress_evidence(",
    "fn controller_static_ingress_evidence(",
    "fn controller_dynamic_ingress_evidence(",
    "fn passive_static_ingress_evidence(",
    "fn passive_dynamic_ingress_evidence(",
]:
    if token not in facts:
        raise SystemExit(
            f"offer route profile violation: ingress evidence derivation must be typed and owner-local: {token}"
        )
for token in [
    "fn offer_controller_readiness(",
    "fn offer_passive_readiness(",
    "fn offer_early_decision_readiness(",
    "fn offer_binding_probe_mode(",
    "fn controller_ingress_evidence(",
    "fn passive_ingress_evidence(",
    "fn controller_readiness(",
    "fn passive_readiness(",
    "fn passive_ack_is_materializable(",
    "fn selected_peer_is_recv(",
    "fn passive_binding_probe_mode(",
    "const fn from_present(present: bool)",
    "const fn from_pending(pending: bool)",
    "const fn from_has_recv(has_recv: bool)",
    "const fn from_materializable(",
    "const fn from_loop_binding_probe(",
    "const fn from_recv_cursor(",
]:
    if token in facts:
        raise SystemExit(
            f"offer route profile violation: profile-specific ingress planners must replace central boolean helpers: {token}"
        )
PY
offer_alignment_source="$(
  cat src/endpoint/kernel/offer/select_alignment.rs
  cat src/endpoint/kernel/offer/select_alignment/candidates.rs
  cat src/endpoint/kernel/offer/select_alignment/model.rs
  cat src/endpoint/kernel/offer/select_alignment/model/*.rs
)"
for required in "enum CurrentOfferEntry" "enum CurrentOfferAuthority" "struct OfferEntrySet" "struct CurrentOfferObservation" "struct OfferAlignmentCandidatePool" "struct ClassifiedOfferCandidateSets" "fn arbitration_frontier(" "enum OfferAlignmentOutcome" "struct OfferAlignmentSelection"; do
  if [[ "${offer_alignment_source}" != *"${required}"* ]]; then
    echo "offer alignment violation: alignment selection must use typed current-entry/authority snapshots and entry-set arbitration: ${required}" >&2
    FAILED=1
  fi
done
for forbidden_model_shape in "observed: u8" "ready: u8" "ready_arm: u8" "controller: u8" "dynamic_controller: u8" "progress: u8" "current: u8" "candidates: u8" "controllers: u8" "dynamic_controllers: u8" "observed: OfferEntryMask" "ready: OfferEntryMask" "ready_arm: OfferEntryMask" "controller: OfferEntryMask" "dynamic_controller: OfferEntryMask" "progress: OfferEntryMask" "current: OfferEntryMask" "pub(super) current_ready" "pub(super) current_progress_evidence" "struct OfferAlignmentCandidateSet"; do
  if [[ "${offer_alignment_source}" == *"${forbidden_model_shape}"* ]]; then
    echo "offer alignment violation: mask storage and current evidence must stay behind typed entry sets: ${forbidden_model_shape}" >&2
    FAILED=1
  fi
done
select_alignment_source="$(cat src/endpoint/kernel/offer/select_alignment.rs src/endpoint/kernel/offer/select_alignment/candidates.rs)"
for forbidden_alignment in \
  "current_matches_candidate" \
  "current_entry_unrunnable" \
  "current_entry_matches_after_filter" \
  "current_entry_is_candidate" \
  "should_suppress_current_passive_without_evidence" \
  "candidate_mask" \
  "observed_mask" \
  "hint_filter_mask" \
  "candidate_count" \
  "dynamic_controller_count" \
  "controller_count" \
  "candidate_idx" \
  "choose_offer_priority"
do
  if [[ "${select_alignment_source}" == *"${forbidden_alignment}"* ]]; then
    echo "offer alignment violation: cursor alignment orchestration must not rebuild boolean/mask arbitration: ${forbidden_alignment}" >&2
    FAILED=1
  fi
done
if grep -Fq "YieldRestart { armed" src/endpoint/kernel/offer/resolve_types.rs \
  || rg -n "yield_armed|YieldRestart \\{ armed" src/endpoint/kernel/offer
then
  echo "offer pending-state violation: yield/restart phases must be enum states, not boolean phase flags" >&2
  FAILED=1
fi

if rg -n "record_loop_decision\\(|record_route_decision_for_scope_lanes\\(|emit_route_decision\\(" \
  src/endpoint/kernel/core/send_control_ops.rs
then
  echo "send-control authority violation: local-control mint must build a decision plan instead of publishing route/loop authority before send commit" >&2
  FAILED=1
fi
send_finish_body="$(
  awk '
    /pub\(crate\) fn finish_send_after_transport_runtime/ { capture=1 }
    capture { print }
    capture && /fn emit_send_after_transport_event/ { exit }
  ' src/endpoint/kernel/core/send_ops.rs
)"
for forbidden_finish in \
  "?;" \
  "preflight_send_control_dispatch(" \
  "build_send_progress_commit_plan(" \
  "build_send_control_decision_plan(" \
  "require_send_progress_commit_plan_after_preflight" \
  "require_send_control_decision_plan_after_preflight" \
  "resolve_send_control_outcome("
do
  if [[ "${send_finish_body}" == *"${forbidden_finish}"* ]]; then
    echo "send-control commit violation: post-transport finish must not expose public fallible preflight: ${forbidden_finish}" >&2
    FAILED=1
  fi
done
	if [[ "${send_finish_body}" != *"finish_send_control_outcome(control)"* \
  || "${send_finish_body}" != *"publish_send_control_decision_plan(decision)"* \
  || "${send_finish_body}" != *"publish_send_progress_commit_plan(meta, progress)"* \
  || "${send_finish_body}" != *"SendCommitOutcome"* \
  || "${send_finish_body}" != *"SendCommitOutcome { descriptor }"* \
  || "${send_finish_body}" == *"publish_send_descriptor("* ]]
then
  echo "send-control commit violation: post-transport finish must publish endpoint-local proofs and return a resident descriptor publication proof" >&2
  FAILED=1
fi
send_control_commit_source="$(cat src/endpoint/kernel/core/send_control_commit.rs)"
carrier_send_source="$(cat src/endpoint/carrier/send.rs)"
flow_source="$(cat src/endpoint/flow.rs)"
descriptor_controls_source="$(
  cat src/control/cluster/core/descriptor_controls.rs
  cat src/control/cluster/core/descriptor_controls/prepared_send.rs
)"
prepared_send_source="$(cat src/control/cluster/core/descriptor_controls/prepared_send.rs)"
if [[ "${send_control_commit_source}" == *"send control dispatch effect must be preflighted before transport publication"* \
  || "${send_control_commit_source}" == *"send control dispatch must be preflighted before transport publication"* \
  || "${send_control_commit_source}" == *"dispatch_descriptor_control_frame("* \
  || "${descriptor_controls_source}" == *"dispatch_descriptor_control_frame("* \
  || "${prepared_send_source}" == *"run_effect("* \
  || "${prepared_send_source}" == *"CpCommand"* \
  || "${descriptor_controls_source}" != *"pub(crate) fn prepare_send_bound_descriptor_terminal"* \
  || "${descriptor_controls_source}" != *"pub(crate) fn publish_descriptor_terminal"* \
  || "${descriptor_controls_source}" != *"pub(crate) fn rollback_descriptor_terminal"* \
  || "${flow_source}" != *"outcome.descriptor.publish();"* \
  || "${flow_source}" != *"Poll<SendResult<()>>"* \
  || "${flow_source}" == *"SendControlOutcome"* \
  || "${carrier_send_source}" == *"fn publish_send_descriptor_public_endpoint"* \
  || "${send_control_commit_source}" != *"rollback_send_descriptor_terminal(proof.descriptor)"* \
  || "${send_control_commit_source}" != *"cluster.rollback_descriptor_terminal(ticket)"* \
  || "${send_control_commit_source}" == *"proof.descriptor.rollback()"* \
  || "${send_control_commit_source}" != *"rollback_send_commit_proof"* ]]
then
  echo "send-control commit violation: post-transport descriptor dispatch must keep resident state ticket-only and rollback through the active endpoint/cluster owner" >&2
  FAILED=1
fi
python3 - <<'PY' || FAILED=1
from pathlib import Path

def is_test_source(path: Path) -> bool:
    parts = path.parts
    name = path.name
    return (
        name == "tests.rs"
        or name.endswith("_tests.rs")
        or "tests" in parts
        or str(path).startswith("src/test_support/")
        or str(path).startswith("src/endpoint/kernel/test_support/")
    )

forbidden = [
    "for_test",
    "transport_for_test",
    "synthetic_for_test",
    "CpCommand",
    "PendingEffect",
    "EffectRunner",
    "DelegateOperands",
    "struct EffectEnvelope {",
    "enum EffectEnvelopeSource",
    "control_op_is_idempotent",
    "control_op_requires_gen_bump",
    "control_op_is_terminal",
    "control_op_modifies_history",
    "emit_policy_event_with_arg2",
    "run_effect_step",
    "after_local_effect",
    "PendingCapRelease::inert",
    "pub(crate) fn inert() -> Self",
    "pub(crate) fn disarm(&mut self)",
    "PolicyEventSpec",
    "PolicyEventKind",
    "TapEvents",
    "TEST_GLOBAL_TAP_RING",
    "TS_CHECKER",
    "install_ts_checker",
]
hits = []
for path in Path("src").rglob("*.rs"):
    if is_test_source(path):
        continue
    source = path.read_text()
    for token in forbidden:
        if token in source:
            hits.append(f"{path}: {token}")
if hits:
    print("test-only residue violation: production source must not retain repo-test effect or offer helpers", file=__import__("sys").stderr)
    for hit in hits:
        print(hit, file=__import__("sys").stderr)
    raise SystemExit(1)
PY
python3 - <<'PY' || FAILED=1
from pathlib import Path

forbidden = [
    "CpCommand",
    "PendingEffect",
    "EffectRunner",
    "DelegateOperands",
    "run_effect_step",
    "after_local_effect",
    "dispatch_topology_ack_with_handle",
    "synthetic_for_test",
    "transport_for_test",
    "NonNull::dangling",
    "receipt: None",
]
hits = []
for path in Path("src").rglob("*.rs"):
    source = path.read_text()
    for token in forbidden:
        if token in source:
            hits.append(f"{path}: {token}")
if hits:
    print(
        "test-only residue violation: source tests must not retain effect runners or fabricate impossible receive-frame states",
        file=__import__("sys").stderr,
    )
    for hit in hits:
        print(hit, file=__import__("sys").stderr)
    raise SystemExit(1)
PY
send_terminal_region="$(cat src/endpoint/kernel/core/send_descriptor_terminal.rs)"
send_publication_region="$(cat src/endpoint/kernel/core/send_descriptor_publication.rs)"
if ! grep -Fq "fn build_send_commit_plan" src/endpoint/kernel/core/send_ops.rs \
  || ! grep -Fq "commit_plan: Some(commit_plan)" src/endpoint/kernel/core/send_ops.rs \
  || ! grep -Fq "descriptor: SendDescriptorTerminal<'rv>" src/endpoint/kernel/core/runtime_types.rs \
  || ! grep -Fq "pub(crate) fn into_ticket(self)" src/endpoint/kernel/core/send_descriptor_terminal.rs \
  || ! grep -Fq "pub(crate) fn publish(self)" src/endpoint/kernel/core/send_descriptor_publication.rs \
  || ! grep -Fq "SendProgressCommitPlan" src/endpoint/kernel/core/runtime_types.rs \
  || ! grep -Fq "SendControlDecisionPlan" src/endpoint/kernel/core/runtime_types.rs \
  || ! grep -Fq "commit_plan: Option<" src/endpoint/kernel/core/runtime_types.rs \
  || grep -Fq "control: commit_plan.control" src/endpoint/kernel/core/send_ops.rs \
  || grep -Fq "commit_proof: Some(commit_plan.proof)" src/endpoint/kernel/core/send_ops.rs \
  || [[ "${send_terminal_region}" == *"preview_cursor_index: Option<StateIndex>"* ]] \
  || [[ "${send_terminal_region}" == *"dispatch: Option<DescriptorDispatch>"* ]] \
  || [[ "${send_publication_region}" == *"preview_cursor_index: Option<StateIndex>"* ]] \
  || [[ "${send_publication_region}" == *"dispatch: Option<DescriptorDispatch>"* ]] \
  || grep -Fq "Committing {" src/endpoint/kernel/core/runtime_types.rs
then
  echo "send-control commit violation: fallible send commit planning must produce compact terminal proofs before transport publication" >&2
  FAILED=1
fi
python3 - <<'PY' || FAILED=1
from pathlib import Path

send_ops = Path("src/endpoint/kernel/core/send_ops.rs").read_text()
start = send_ops.find("fn build_send_commit_plan(")
end = send_ops.find("\n    #[inline(never)]\n    fn prepare_send_payload_plan", start)
if start < 0 or end < 0:
    raise SystemExit("send-control commit violation: build_send_commit_plan body must be bounded")
body = send_ops[start:end]
progress = body.find("self.build_send_progress_commit_plan(")
decision = body.find("self.build_send_control_decision_plan(")
reserve = body.find("self.reserve_descriptor_terminal_for_send(")
if min(progress, decision, reserve) < 0 or not (progress < reserve and decision < reserve):
    raise SystemExit("send-control commit violation: descriptor reservation must be the final fallible authority acquisition")
command_types = "".join(Path(path).read_text() for path in ("src/control/cluster/core/command_types.rs", "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal.rs", "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/topology.rs", "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/lane_effect.rs", "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/publisher.rs"))
descriptor_controls = Path("src/control/cluster/core/descriptor_controls.rs").read_text()
prepared_owner = "".join(Path(path).read_text() for path in ("src/control/cluster/core/descriptor_controls/prepared_send.rs", "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_effects.rs"))
prepared_effects = Path("src/rendezvous/core/prepared_effects.rs").read_text()
snapshot_table = "".join(Path(path).read_text() for path in ("src/rendezvous/tables/snapshot.rs", "src/rendezvous/tables/snapshot/reservation.rs"))
lane_effects = "".join(Path(path).read_text() for path in ("src/rendezvous/core/lane_lifecycle/prepared_effects.rs", "src/rendezvous/core/topology_process.rs"))
required_command = ("pub(crate) struct DescriptorTerminal {", "pub(crate) struct DescriptorPublicationAuthority", "ops: &'static DescriptorPublicationAuthorityOps", "struct DescriptorPublicationAuthorityOps", "publish: unsafe fn(*const (), DescriptorTerminal)", "ReservedTopology(", "DescriptorEffectTerminal(", "pub(super) enum DescriptorTerminalCase", "pub(super) enum ReservedTopologyTerminal", "pub(super) struct ReservedTopologyCommitPublication", "pub(super) enum DescriptorEffectTerminal", "pub(super) struct PreparedDescriptorEffect<Proof>", "AbortBegin(PreparedDescriptorEffect<PreparedAbortBeginEffect>)", "TxAbort(PreparedDescriptorEffect<PreparedTxAbortEffect>)")
required_prepared = ("fn publish_descriptor_effect_terminal(", "fn rollback_descriptor_effect_terminal_in_core(", "publish_prepared_abort_begin_effect(proof)", "publish_prepared_tx_abort_effect(proof)", "rollback_prepared_tx_abort_effect(proof)")
required_snapshot = ("reservation: PreparedSnapshotFinalization", "reservation: PreparedSnapshotRecord", "pub(crate) fn reserve_record(", "pub(crate) fn publish_record_reserved(", ") -> PublishedSnapshotRecord", "pub(crate) fn rollback_record_reserved(", "pub(crate) fn reserve_finalization(", "pub(crate) fn publish_finalization_reserved(", ") -> PublishedSnapshotFinalization", "pub(crate) fn rollback_finalization_reserved(")
forbidden_command = ("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct DescriptorTerminal", "pub(crate) enum DescriptorTerminal {", "DescriptorTerminalKind", "DescriptorEffectEvidence", "op: ControlOp", "fn op(&self)", "pub(super) enum DescriptorEffect {", "reserved_topology(", "lane_effect_evidence(", "fn kind(&self)", "terminal: unsafe fn(*const (), DescriptorTerminal, bool)", "publish: bool", "rollback: unsafe fn(*const (), DescriptorTerminal)")
if (
    any(item not in command_types for item in required_command)
    or any(item not in prepared_owner for item in required_prepared)
    or any(item not in prepared_effects + snapshot_table for item in required_snapshot)
    or "ensure_associated_session_lane(sid, lane)" not in lane_effects
    or "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct Prepared" in prepared_effects
    or any(item in lane_effects for item in ("record_snapshot(", "mark_committed(", "mark_restored("))
    or any(item in command_types for item in forbidden_command)
):
    raise SystemExit("send-control commit violation: descriptor terminal must split topology proof from affine descriptor-effect reservations through a compact ops table without a kind-tag carrier or boolean terminal dispatcher")
for function, consume, side_effect in (("fn publish_prepared_state_snapshot_effect", "publish_record_reserved(proof.into_reservation())", "discard_released_lane_entries(lane)"), ("fn publish_prepared_tx_commit_effect", "publish_finalization_reserved(proof.into_reservation())", "discard_released_lane_entries(lane)"), ("fn publish_prepared_state_restore_effect", "publish_finalization_reserved(proof.into_reservation())", "self.r#gen.publish_prepared(lane, generation)"), ("fn publish_prepared_tx_abort_effect", "publish_finalization_reserved(proof.into_reservation())", "self.r#gen.publish_prepared(lane, generation)")):
    start = lane_effects.find(function)
    if start < 0:
        raise SystemExit(f"send-control commit violation: {function} must exist")
    rest = lane_effects[start:]
    marker = rest.find("\n    #[inline]", len(function))
    body = rest[: marker if marker >= 0 else len(rest)]
    consume_at = body.find(consume)
    side_effect_at = body.find(side_effect)
    if consume_at < 0 or side_effect_at < 0 or consume_at > side_effect_at:
        raise SystemExit(f"send-control commit violation: {function} must consume the prepared reservation before terminal side effects")
if (
    "mod descriptor_terminal;" in descriptor_controls or "mod prepared_topology_commit;" in descriptor_controls
    or "mod descriptor_terminal;" not in prepared_owner
    or "mod topology_commit_rollback;" not in prepared_owner
    or "pub(super) fn topology_begin" not in command_types
    or "pub(super) fn topology_ack" not in command_types
    or "pub(super) fn commit_topology" not in command_types
    or "pub(super) const fn abort_begin" not in command_types
    or "pub(super) const fn abort_ack" not in command_types
    or "pub(super) const fn state_snapshot" not in command_types
    or "pub(super) const fn state_restore" not in command_types
    or "pub(super) const fn tx_commit" not in command_types
    or "pub(super) const fn tx_abort" not in command_types
    or "pub(in crate::control::cluster::core) const fn topology_" in command_types
    or "pub(in crate::control::cluster::core) const fn abort_" in command_types
    or "pub(in crate::control::cluster::core) const fn state_" in command_types
    or "pub(in crate::control::cluster::core) const fn tx_" in command_types
    or "pub(in crate::control::cluster::core::descriptor_controls) const fn topology_" in command_types
    or "pub(in crate::control::cluster::core::descriptor_controls) const fn abort_" in command_types
    or "pub(in crate::control::cluster::core::descriptor_controls) const fn state_" in command_types
    or "pub(in crate::control::cluster::core::descriptor_controls) const fn tx_" in command_types
    or "pub(in crate::control::cluster::core::descriptor_controls) enum DescriptorTerminalCase" in command_types or "pub(in crate::control::cluster::core::descriptor_controls) enum ReservedTopologyTerminal" in command_types or "pub(in crate::control::cluster::core::descriptor_controls) struct ReservedTopology" in command_types or "pub(in crate::control::cluster::core::descriptor_controls) struct DescriptorEffectTerminal" in command_types or "pub(in crate::control::cluster::core::descriptor_controls) fn into_" in command_types
    or "DescriptorTerminal::topology_begin" not in prepared_owner
    or "DescriptorTerminal::topology_ack" not in prepared_owner
    or "DescriptorTerminal::commit_topology" not in prepared_owner
):
    raise SystemExit("send-control commit violation: descriptor terminal constructors must be nested under the descriptor prepare owner")

prepared = Path("src/control/cluster/core/descriptor_controls/prepared_send.rs").read_text()
prepared_commit = prepared + Path("src/control/cluster/core/descriptor_controls/prepared_send/topology_commit_rollback.rs").read_text()
local_prepared = Path("src/rendezvous/core/local_topology/prepared_commit.rs").read_text()
local_commit_reservation = Path("src/rendezvous/topology/commit_reservation.rs").read_text() + Path("src/rendezvous/topology/commit_reservation/destination.rs").read_text()
required_topology_publish_guards = ["publish_prepared_begin(", "publish_prepared_ack(", "publish_prepared_commit("]
required_commit_guards = ["reserve_source_topology_commit(", "reserve_destination_topology_commit(", "assert_prepared_source_topology_commit(", "assert_prepared_destination_topology_commit(", "assert_prepared_commit(&distributed)", "publish_prepared_source_topology_commit(", "publish_prepared_destination_topology_commit(", "rollback_source_topology_commit_reservation(", "rollback_destination_topology_commit_reservation("]
forbidden_prepared = ["publish_begin_reserved(", "publish_ack_reserved(", "rollback_ack_reserved(", "get_from(sid, src_rv)", "preflight_commit_reserved(", "ensure_commit_reserved(", "consume_prepared_commit(", "preflight_prepared_topology_commit(", "prepared_topology_commit_ready(", "prepared_topology_commit_owners_present(", "ensure_reserved_source_topology_commit(", "ensure_reserved_destination_topology_commit(", "consume_reserved_source_topology_commit(", "consume_reserved_destination_topology_commit(", "publish_reserved_source_topology_commit(", "publish_reserved_destination_topology_commit("]
forbidden_commit = ["publish_commit_reserved(", "topology_commit(sid, source_lane).is_err()"]
if any(item not in prepared for item in required_topology_publish_guards) or any(item not in prepared_commit for item in required_commit_guards) or any(item in prepared for item in forbidden_prepared) or any(item in prepared_commit for item in forbidden_commit):
    raise SystemExit("send-control commit violation: prepared topology publish must mint and consume Begin/Ack/Commit owner proofs")
destination_consume = prepared.find(".publish_prepared_destination_topology_commit(destination, meta.dst_lane())")
source_consume = prepared.find("(&mut *src_ptr).publish_prepared_source_topology_commit(")
distributed = prepared.find("publish_prepared_commit(distributed)")
distributed_assert = prepared.find("assert_prepared_commit(&distributed)")
destination_assert = prepared.find("assert_prepared_destination_topology_commit(")
source_assert = prepared.find("assert_prepared_source_topology_commit(")
distributed_success = prepared.find("Distributed commit proof is consumed;")
if (
    source_consume < 0
    or destination_consume < 0
    or distributed < 0
    or distributed_assert < 0
    or destination_assert < 0
    or source_assert < 0
    or distributed_success < 0
    or not (distributed_assert < destination_assert < source_assert < distributed)
    or distributed > distributed_success
    or distributed_success > destination_consume
    or destination_consume > source_consume
):
    raise SystemExit("send-control commit violation: topology commit invariants must be asserted before distributed terminal proof consumption and local proof publication")
pre_distributed = prepared[:distributed]
if (
    "publish_prepared_destination_topology_commit(" in pre_distributed
    or "publish_prepared_source_topology_commit(" in pre_distributed
):
    raise SystemExit("send-control commit violation: topology commit publish must not mutate local topology before distributed terminal proof consumption")
post_distributed = prepared[distributed_success:destination_consume]
if (
    "return;" in post_distributed
    or "get_pair_mut(" in post_distributed
    or "owners disappeared" in post_distributed
):
    raise SystemExit("send-control commit violation: topology commit publish must not keep owner lookup or early-return paths after distributed terminal proof consumption")
post_destination = prepared[destination_consume:source_consume]
if (
    "return;" in post_destination
    or "prepared source rendezvous disappeared" in post_destination
    or "prepared destination rendezvous disappeared" in post_destination
):
    raise SystemExit("send-control commit violation: topology commit publish must not keep an early-return path after local proof consumption")
for publish in ["publish_prepared_destination_topology_commit", "publish_prepared_source_topology_commit"]:
    start = local_prepared.find(f"fn {publish}(")
    if start < 0:
        raise SystemExit(f"send-control commit violation: missing local topology proof publisher {publish}")
    next_fn = local_prepared.find("\n    pub(crate) fn ", start + 1)
    body = local_prepared[start : next_fn if next_fn >= 0 else len(local_prepared)]
    for forbidden in ["assert!", "assert_eq!", ".is_err()", ".unwrap()"]:
        if forbidden in body:
            raise SystemExit(
                "send-control commit violation: local prepared topology commit publish must not hide fail-closed branches behind proof consumption"
            )
for fn_name in ["rollback_source_commit_reserved", "rollback_destination_commit_reserved", "assert_source_commit_reserved", "assert_destination_commit_reserved", "clear_prepared_source_commit_unchecked", "finalize_prepared_destination_commit_unchecked"]:
    start = local_commit_reservation.find(f"fn {fn_name}(")
    if start < 0:
        raise SystemExit(f"send-control commit violation: missing local topology proof consumer {fn_name}")
    next_fn = local_commit_reservation.find("\n    pub(in crate::rendezvous) fn ", start + 1)
    body = local_commit_reservation[start : next_fn if next_fn >= 0 else len(local_commit_reservation)]
    if "debug_assert" in body or "return;" in body:
        raise SystemExit(
            "send-control commit violation: local topology proof consumers must assert invariants in release, not debug-return"
        )
PY
if ! grep -Fq "EndpointTx policy audit is an attempt-side replay tuple" src/endpoint/kernel/core/send_ops.rs; then
  echo "endpoint policy audit violation: EndpointTx audit must be explicitly documented as an attempt-side replay tuple" >&2
  FAILED=1
fi
topology_ack_mint_body="$(
  awk '
    /fn mint_local_topology_ack_control/ { capture=1 }
    capture { print }
    capture && /fn mint_control_token_bytes_with_handle/ { exit }
  ' src/endpoint/kernel/core/send_control_ops.rs
)"
if [[ "${topology_ack_mint_body}" != *"cached_topology_operands(cp_sid)"* \
  || "${topology_ack_mint_body}" == *"take_cached_topology_operands"* ]]
then
  echo "send-control topology violation: topology ack mint must peek cached operands and leave consume to dispatch success" >&2
  FAILED=1
fi
if ! grep -Fq "cached_operands_remove(sid)" src/control/cluster/core/descriptor_controls/prepared_send.rs; then
  echo "send-control topology violation: TopologyAck effect success must consume cached operands" >&2
  FAILED=1
fi
if ! grep -Fq "enum SendControlDecisionPlan" src/endpoint/kernel/core/public_types.rs \
  || ! grep -Fq "fn publish_send_control_decision_plan" src/endpoint/kernel/core/send_control_commit.rs
then
  echo "send-control authority violation: committed local control decisions must have a typed post-dispatch publish owner" >&2
  FAILED=1
fi

lease_bundle_source="$(cat src/control/lease/bundle.rs)"
if [[ "${lease_bundle_source}" == *"CapsRollbackAuthority"* \
  || "${lease_bundle_source}" == *"CapsBundleHandle"* \
  || "${lease_bundle_source}" == *"track_mint"* \
  || "${lease_bundle_source}" == *"release_by_nonce"* \
  || "${lease_bundle_source}" == *"NonNull<CapTable>"* ]]
then
  echo "lease cap rollback violation: lease bundle must not advertise an inert cap rollback owner" >&2
  FAILED=1
fi
lease_planner_source="$(cat src/control/lease/planner.rs)"
if [[ "${lease_planner_source}" == *"FACET_CAPS"* \
  || "${lease_planner_source}" == *"requires_caps"* \
  || "${lease_planner_source}" == *"facets_caps"* \
  || "${lease_planner_source}" == *"facets: LeaseFacetNeeds"* \
  || "${lease_planner_source}" == *"self.facets"* \
  || "${lease_planner_source}" == *"req.facets"* \
  || "${lease_planner_source}" == *"with_facets"* \
  || "${lease_planner_source}" == *"\"caps\""* \
  || "$(cat src/control/cluster/core/endpoint_attach.rs)" == *"slot/caps/topology"* ]]
then
  echo "lease facet violation: lease planner must not retain ghost cap facets or inert facet aggregation after cap rollback moved to token drop guards" >&2
  FAILED=1
fi
lease_core_source="$(cat src/control/lease/core.rs)"
lease_bundle_source="$(cat src/control/lease/bundle.rs)"
if [[ "${lease_core_source}" == *"LeaseObserve"* \
  || "${lease_bundle_source}" == *"LeaseObserve"* \
  || "${lease_core_source}" == *"from_resident_tap"* \
  || "${lease_bundle_source}" == *"observe: Option<LeaseObserve"* \
  || "${lease_bundle_source}" == *"commit_event: Option<TapEvent>"* \
  || "${lease_bundle_source}" == *"rollback_event: Option<TapEvent>"* \
  || "${lease_core_source}" == *"pub(crate) const fn new(tap: *const TapRing"* ]]
then
  echo "lease observe violation: unused observe/tap authority must be deleted rather than hidden behind test cfg" >&2
  FAILED=1
fi

if grep -Fq "pub(crate) enum CapError" src/rendezvous/error.rs \
  || grep -Fq "fn map_token_cap_error" src/rendezvous/core/cap_ledger.rs
then
  echo "capability ledger owner violation: rendezvous must use the canonical integration CapError without a mirror enum or mapper" >&2
  FAILED=1
fi
if [[ -e src/rendezvous/core/cap_claim.rs ]]; then
  echo "capability ledger owner violation: stale cap_claim module path must not remain as a compatibility layer" >&2
  FAILED=1
fi
capability_ledger_sources="$(cat src/rendezvous/core/cap_ledger.rs src/rendezvous/capability.rs src/control/cap/mint.rs src/control/cap/mint/error.rs src/control/cap/mint/resource.rs src/observe.rs src/observe/ids.rs)"
for forbidden in \
  "fn mint_cap" \
  "fn claim_cap" \
  "claim_by_nonce" \
  "ClaimableResourceKind" \
  "cap_mint_id" \
  "cap_claim_id" \
  "cap_exhaust_id" \
  "CAP_MINT_BASE" \
  "CAP_CLAIM_BASE" \
  "CAP_EXHAUST_BASE" \
  "Exhausted"
do
  if [[ "${capability_ledger_sources}" == *"${forbidden}"* ]]; then
    echo "capability ledger owner violation: deleted mint/claim compatibility surface remains: ${forbidden}" >&2
    FAILED=1
  fi
done

if grep -Fq "    lane_idx: usize," src/rendezvous/port/recv_frame.rs; then
  echo "received frame owner violation: ReceivedFrame must retain wire lane identity and derive usize indices only at use sites" >&2
  FAILED=1
fi
if grep -Fqi "current epoch" src/rendezvous/port/recv_frame.rs; then
  echo "received frame contract violation: requeue safety comments must describe receipt ownership, not stale epoch authority" >&2
  FAILED=1
fi

capability_token_source="$(cat src/control/cap/mint.rs src/control/cap/mint/token.rs)"
for forbidden in \
  "thread_local!" \
  "[u8; 6]" \
  "#[derive(Debug, PartialEq, Eq)]"
do
  if [[ "${capability_token_source}" == *"${forbidden}"* ]]; then
    echo "capability surface violation: public token docs/debug must be no_std-friendly and opaque: ${forbidden}" >&2
    FAILED=1
  fi
done
if [[ "${capability_token_source}" == *".field(\"bytes\""* ]]; then
  echo "capability surface violation: GenericCapToken Debug must not expose token bytes" >&2
  FAILED=1
fi
for required in \
  "fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN]" \
  "impl<K: ResourceKind> fmt::Debug for GenericCapToken<K>"
do
  if [[ "${capability_token_source}" != *"${required}"* ]]; then
    echo "capability surface violation: missing no_std/opaque token contract: ${required}" >&2
    FAILED=1
  fi
done

capability_source="$(cat src/rendezvous/capability.rs src/endpoint/kernel/core/public_types.rs src/endpoint/kernel/core/send_control_commit.rs)"
if [[ "${capability_source}" != *"pub(crate) struct CapReleaseCtx<'rv>"* || "${capability_source}" != *"cap_table: &'rv CapTable"* || "${capability_source}" != *"snapshots: &'rv StateSnapshotTable"* || "${capability_source}" != *"revisions: &'rv Cell<u64>"* || "${capability_source}" != *"release_ctx: Option<CapReleaseCtx<'rv>>"* || "${capability_source}" != *"pub(crate) fn release_now(mut self)"* || "${capability_source}" != *"release.release_now();"* ]]; then
  echo "capability release violation: CapReleaseCtx and drop guards must carry the rendezvous lifetime and commit through direct release_now instead of token-byte roundtrips" >&2
  FAILED=1
fi
if [[ "${capability_source}" == *"NonNull<CapTable>"* || "${capability_source}" == *"NonNull<StateSnapshotTable>"* || "${capability_source}" == *"NonNull<Cell<u64>>"* || "${capability_source}" == *"#[derive(Clone, Copy)]"$'\n'"pub(crate) struct CapReleaseCtx"* || "${capability_source}" == *"RawRegisteredCapToken"* || "${capability_source}" == *"into_registered_token("* || "${capability_source}" == *"send_control_token_bytes("* ]]; then
  echo "capability release violation: CapReleaseCtx is a consuming release context and must not erase ownership or recreate registered token bytes after transport success" >&2
  FAILED=1
fi
if grep -Fq "let claim_revision = self.next_cap_revision();" \
  src/rendezvous/core/cap_ledger.rs
then
  echo "capability ledger violation: failed capability validation must not advance the revision clock" >&2
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
