use super::common::read;

#[test]
fn direct_recv_commits_only_through_observed_evidence_plan() {
    let recv = read("src/endpoint/kernel/recv.rs");
    let recv_matching = read("src/endpoint/kernel/recv/matching.rs");
    let recv_evidence = read("src/endpoint/kernel/recv/evidence.rs");
    let inbound_key = read("src/global/typestate/facts/inbound_key.rs");
    let unique_match = read("src/runtime_core/unique_match.rs");
    let recv_commit_plan = read("src/endpoint/kernel/recv_commit_plan.rs");
    let core = read("src/endpoint/kernel/core.rs");
    let lane_port = read("src/endpoint/kernel/lane_port.rs");
    let observe = read("src/endpoint/kernel/observe.rs");
    let surface = [
        recv.as_str(),
        recv_matching.as_str(),
        recv_evidence.as_str(),
        inbound_key.as_str(),
        unique_match.as_str(),
        recv_commit_plan.as_str(),
        core.as_str(),
    ]
    .join("\n");

    for required in [
        "struct InboundFrameKey",
        "pub(crate) source_role: u8,",
        "pub(crate) lane: u8,",
        "pub(crate) frame_label: u8,",
        "struct DeterministicInboundKey",
        "pub(crate) schema: u32,",
        "key.matches_recv(meta)",
        "enum UniqueMatch<T>",
        "None,\n    One(T),\n    Ambiguous,",
        "enum UniqueMatchFailure",
        "if lane_wire != meta.lane",
        "observed.matches_recv(meta)",
        "pub(super) struct RecvCommitPlan<'r>",
        "enum RecvCommitPlanKind",
        "RecvCommitPlanKind::Direct",
        "RecvCommitPlanKind::Branch { branch }",
        "fn prepare_recv_commit_delta(",
        "frame.discard_uncommitted();",
        "fn poll_recv_preamble_for_label(",
        "fn unique_recv_candidate(",
        "fn unique_deterministic_recv_candidate(",
        "fn accept_framed_recv_frame(",
        "fn accept_deterministic_recv_frame(",
        "fn accept_recv_frame(",
        "fn poll_recv_kernel_frame_source(",
        "fn finish_recv_kernel_frame(",
        "fn publish_recv_commit_plan<F>(",
        "Self::Wire(frame) => frame.validated_payload(validate).map(|_| ())",
        "Self::Wire(frame) => frame.into_payload()",
    ] {
        assert!(
            surface.contains(required),
            "direct recv must keep observed-evidence commit authority: {required}"
        );
    }

    let preamble_surface = [lane_port.as_str(), observe.as_str(), recv.as_str()].join("\n");
    for required in [
        "PreambleObservation",
        "PreambleFrame::from_deterministic_payload(",
        "match observed {",
        "None => Poll::Ready(Ok(PreambleFrame::from_deterministic_payload(",
        "poll_received_framed_transport_frame_for_lane(",
        "if frame.is_deterministic()",
        "FrameMismatch::headerless_preamble(",
    ] {
        assert!(
            preamble_surface.contains(required),
            "deterministic recv must keep private preamble observation and framed-only offer boundary: {required}"
        );
    }

    let preamble_poll = lane_port
        .split("fn poll_recv_frame_preamble")
        .nth(1)
        .and_then(|tail| tail.split("fn poll_recv_payload").next())
        .expect("preamble poll helper must stay visible");
    assert!(
        !preamble_poll.contains("FrameMismatch::headerless_preamble"),
        "direct recv preamble polling must not reject deterministic headerless frames before candidate scan"
    );

    assert!(
        recv_evidence.contains("struct RecvCandidate {\n    pub(in crate::endpoint::kernel::recv) desc: RecvDescriptor,\n}"),
        "RecvCandidate must be identity-only and must not carry references or codec hooks"
    );
    assert!(
        recv_evidence.contains(
            "struct RecvDescriptor {\n    pub(in crate::endpoint::kernel::recv) meta: RecvMeta,\n    pub(in crate::endpoint::kernel::recv) cursor_index: StateIndex,\n}"
        ),
        "RecvDescriptor must not duplicate descriptor-derived lane identity"
    );

    for forbidden in [
        "PreparedRecv",
        "struct RecvRuntimeDesc",
        "RecvRuntimeDesc::",
        "prepare_recv_descriptor",
        "prepare_recv_kernel_descriptor",
        "poll_recv_kernel_payload_source",
        "finish_recv_kernel_payload",
        "payload_validate",
        "payload: Payload<'a>",
        "ObservedInboundKey",
        "MatchAccumulator",
    ] {
        assert!(
            !surface.contains(forbidden),
            "direct recv must not regain descriptor-first or candidate-codec residue: {forbidden}"
        );
    }
}

#[test]
fn offer_admission_requires_one_enabled_descriptor_with_full_identity() {
    let select = read("src/endpoint/kernel/offer/select.rs");
    let observed = read("src/endpoint/kernel/offer/select_observed.rs");
    let roll = read("src/global/typestate/cursor/scope_route/roll.rs");

    let event_progress = read("src/global/typestate/cursor/scope_route/event_progress.rs");
    assert!(
        select.contains("self.select_observed_ingress_route_scope")
            && !select.contains("select_current_materialized_ingress_scope")
            && observed.contains("!key.matches_recv(meta)")
            && observed.contains(".event_enabled(idx, meta.into(), &mut selected)")
            && observed.contains("UniqueMatch::NONE")
            && observed.contains("matched.is_ambiguous()")
            && observed.contains(".finish_optional()")
            && observed.contains(".passive_descendant_target_index_for_key(scope, key)")
            && observed.contains(".route_arm_lane_first_step(scope, arm, meta.lane)")
            && !observed.contains("idx == current_idx"),
        "current and elastic receives must share descriptor eligibility and reject ambiguous full-key matches"
    );
    assert!(
        !observed.contains("active_reentry_scope_for_observed_frame")
            && !observed.contains(".or_else(")
            && !observed.contains("return self.select_carried_ingress_scope(")
            && observed.contains("info = self.decision_state.lane_offer_state(lane_idx);")
            && observed.contains("state_index_to_usize(info.entry) != self.cursor.index()"),
        "observed ingress must revalidate the descriptor owner without a separate unchecked reentry path"
    );

    assert!(
        !roll.contains("fn roll_reentry_recv_index_for_frame")
            && roll.contains("EventArmView::Committed")
            && roll.contains("EventArmView::Preview")
            && event_progress.contains("!self.event_progress_passed(progress_step)")
            && event_progress
                .contains("self.roll_reentry_event_allows_index(idx, event.lane, arm_for_scope)"),
        "past unchosen events require a fresh visit; candidate conflict preview must retain committed completion history"
    );
}
