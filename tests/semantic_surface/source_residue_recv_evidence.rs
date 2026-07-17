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
fn offer_admission_prioritizes_the_unconsumed_current_frontier() {
    let select = read("src/endpoint/kernel/offer/select.rs");
    let observed = read("src/endpoint/kernel/offer/select_observed.rs");
    let roll = read("src/global/typestate/cursor/scope_route/roll.rs");

    let current = select
        .find("self.select_current_materialized_ingress_scope")
        .expect("offer selection must inspect the current materialized frontier");
    let reentry = select
        .find("self.select_observed_ingress_route_scope")
        .expect("offer selection must retain elastic reentry admission");
    assert!(
        current < reentry,
        "the exact unconsumed current occurrence must precede next-iteration reentry admission"
    );
    assert!(
        observed.contains("self.cursor.node_event_done_for_lane(current_idx, key.lane)"),
        "a consumed current occurrence must not shadow a legal rolled reentry frame"
    );
    assert!(
        observed.contains("match self.active_reentry_scope_for_observed_frame(key)")
            && observed.contains("Some(active_reentry) => Some(active_reentry)")
            && !observed.contains(".or_else(")
            && !observed.contains("return self.select_carried_ingress_scope(")
            && observed.contains("info = self.decision_state.lane_offer_state(lane_idx);")
            && observed.contains("state_index_to_usize(info.entry) != self.cursor.index()"),
        "observed ingress must name reentry precedence and revalidate one exact owner after non-recursive realignment"
    );

    let roll_admission = roll
        .split("pub(crate) fn roll_reentry_recv_index_for_frame")
        .nth(1)
        .and_then(|body| {
            body.split("pub(crate) fn roll_reentry_event_allows_index")
                .next()
        })
        .expect("roll frame admission function must remain inspectable");
    let current_progress = roll_admission
        .find("RollLaneAdmission::Progress")
        .expect("roll frame admission must name current progress");
    let next_head = roll_admission
        .find("RollLaneAdmission::Head")
        .expect("roll frame admission must name the next iteration head");
    assert!(
        current_progress < next_head,
        "same-color roll admission must exhaust current progress before considering a next-iteration head"
    );
}
