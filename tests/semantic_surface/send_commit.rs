use super::common::*;

fn runtime_types_source() -> String {
    let mut source = read("src/endpoint/kernel/core/runtime_types.rs");
    source.push_str(&read("src/endpoint/kernel/core/runtime_types/commit.rs"));
    source
}

#[test]
fn send_finish_after_transport_has_no_public_fallible_preflight() {
    let send_ops = read("src/endpoint/kernel/core/send_ops.rs");
    let finish_start = send_ops
        .find("pub(crate) fn finish_send_after_transport_runtime")
        .expect("finish_send_after_transport_runtime must exist");
    let finish_body = &send_ops[finish_start..];
    let finish_body = &finish_body[..finish_body
        .find("\n    #[inline(never)]\n    fn emit_send_after_transport_event")
        .expect("finish body must end before send event helper")];

    for forbidden in [
        "?;",
        "preflight_send_control_dispatch(",
        "build_send_progress_commit_plan(",
        "build_send_control_decision_plan(",
        "require_send_progress_commit_plan_after_preflight",
        "require_send_control_decision_plan_after_preflight",
        "resolve_send_control_outcome(",
    ] {
        assert!(
            !finish_body.contains(forbidden),
            "post-transport send finish must not expose public fallible preflight: {forbidden}"
        );
    }
    let send_control_commit = read("src/endpoint/kernel/core/send_control_commit.rs");
    let descriptor_controls = format!(
        "{}\n{}",
        read("src/control/cluster/core/descriptor_controls.rs"),
        read("src/control/cluster/core/descriptor_controls/prepared_send.rs")
    );
    for forbidden in [
        ".expect(\n                \"send control dispatch effect must be preflighted before transport publication\"",
        "send control dispatch must be preflighted before transport publication",
        "dispatch_descriptor_control_frame(",
    ] {
        assert!(
            !send_control_commit.contains(forbidden) && !descriptor_controls.contains(forbidden),
            "post-transport descriptor dispatch must consume prepared tickets instead of hidden fallible dispatch: {forbidden}"
        );
    }
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");
    let carrier_send = read("src/endpoint/carrier/send.rs");
    let flow = read("src/endpoint/flow.rs");
    for forbidden in ["run_effect(", "CpCommand"] {
        assert!(
            !prepared_send.contains(forbidden),
            "prepared send descriptor publish must use owner-specific tickets, not generic control dispatch: {forbidden}"
        );
    }
    assert!(
        descriptor_controls.contains("pub(crate) fn prepare_send_bound_descriptor_terminal")
            && descriptor_controls.contains("pub(crate) fn publish_descriptor_terminal")
            && descriptor_controls.contains("pub(crate) fn rollback_descriptor_terminal")
            && flow.contains("outcome.descriptor.publish();")
            && flow.contains("Poll<SendResult<()>>")
            && !flow.contains("SendControlOutcome")
            && send_control_commit
                .contains("let (control, descriptor) = plan.into_rollback_parts();")
            && send_control_commit.contains("self.rollback_send_descriptor_terminal(descriptor);")
            && send_control_commit.contains("drop(control);")
            && send_control_commit.contains("cluster.rollback_descriptor_terminal(ticket)")
            && !send_control_commit.contains("rollback_send_commit_proof")
            && !send_control_commit.contains("plan.map(|plan| plan.proof)"),
        "send descriptor effects must keep endpoint-resident state ticket-only, publish through the post-kernel publication owner, and rollback through the active endpoint/cluster owner"
    );
    let rollback_plan_start = send_control_commit
        .find("pub(in crate::endpoint::kernel) fn rollback_send_commit_plan(")
        .expect("rollback_send_commit_plan must exist");
    let rollback_plan_body = &send_control_commit[rollback_plan_start..];
    assert!(
        rollback_plan_body
            .find("let (control, descriptor) = plan.into_rollback_parts();")
            .expect("send rollback must split commit plan without dropping control first")
            < rollback_plan_body
                .find("self.rollback_send_descriptor_terminal(descriptor);")
                .expect("send rollback must rollback descriptor before cap release")
            && rollback_plan_body
                .find("self.rollback_send_descriptor_terminal(descriptor);")
                .expect("send rollback must rollback descriptor before cap release")
                < rollback_plan_body.find("drop(control);").expect(
                    "send rollback must release registered control after descriptor rollback"
                ),
        "send rollback must rollback descriptor terminal before releasing registered local control authority"
    );

    let control = finish_body
        .find("self.finish_send_control_outcome(control);")
        .expect("post-transport finish must consume the preflighted control emission");
    let publish = finish_body
        .find("self.publish_send_progress_commit_plan(progress);")
        .expect("endpoint progress publish must use the prebuilt progress proof");
    let descriptor = finish_body
        .rfind("SendCommitOutcome { descriptor }")
        .expect("descriptor ticket must be returned for carrier-level publication after kernel borrow closes");

    assert!(
        control < publish && publish < descriptor,
        "post-transport finish must commit endpoint-local state and return descriptor publication to the carrier after the kernel borrow closes"
    );
    let progress_publish_start = send_ops
        .find("fn publish_send_progress_commit_plan(&mut self, plan: SendProgressCommitPlan)")
        .expect("send progress commit helper must exist");
    let progress_publish_body = &send_ops[progress_publish_start..finish_start];
    let delta_apply = progress_publish_body
        .find("let committed = self.commit_prepared_delta(plan.delta);")
        .expect("send progress publish must apply the prepared CommitDelta");
    let route_evidence = progress_publish_body
        .find("self.publish_send_route_evidence_delta(&committed);")
        .expect("send route evidence must be published after prepared CommitDelta apply");
    let event = progress_publish_body
        .find("self.emit_send_after_transport_event(&committed);")
        .expect("send event must be emitted only after endpoint progress publish");
    assert!(
        delta_apply < route_evidence && route_evidence < event,
        "send event emission must follow the single CommitDelta apply boundary"
    );
    assert!(
        !finish_body.contains("publish_send_descriptor(")
            && !flow.contains("publish_descriptor_after_terminal_poll")
            && !carrier_send.contains("fn publish_send_descriptor_public_endpoint")
            && finish_body.contains("SendDescriptorPublication::new(")
            && send_control_commit.contains(".control\n            .cluster()"),
        "descriptor publication authority must be attached only to the post-kernel outcome; endpoint-resident pending send state must keep only the affine descriptor ticket"
    );

    let build_start = send_ops
        .find("fn build_send_commit_plan(")
        .expect("build_send_commit_plan must exist");
    let build_body = &send_ops[build_start
        ..send_ops[build_start..]
            .find(&format!(
                "\n    #[inline(never)]\n    fn {}",
                "prepare_send_payload_plan"
            ))
            .expect("build_send_commit_plan body must be bounded")
            + build_start];
    let progress_build = build_body
        .find("self.build_send_progress_commit_plan(")
        .expect("endpoint progress proof must be built before descriptor reservation");
    let loop_row_build = build_body
        .find("self.build_send_loop_commit_row(")
        .expect("loop commit row must be built before descriptor reservation");
    let descriptor_reserve = build_body
        .find("self.reserve_descriptor_terminal_for_send(")
        .expect("descriptor reservation must be present");
    assert!(
        loop_row_build < progress_build && progress_build < descriptor_reserve,
        "descriptor reservation must be the final fallible authority acquisition in send commit planning"
    );

    let begin_start = send_ops
        .find("fn begin_send_transport(")
        .expect("begin_send_transport must exist");
    let begin_body = &send_ops[begin_start
        ..send_ops[begin_start..]
            .find("\n    #[inline(never)]\n    pub(crate) fn poll_send_init")
            .expect("begin_send_transport body must be bounded")
            + begin_start];
    assert!(
        begin_body.contains("self.build_send_commit_plan(")
            && begin_body.contains("commit_plan: Some(commit_plan)")
            && !begin_body.contains("control: commit_plan.control")
            && !begin_body.contains("commit_proof: Some(commit_plan.proof)")
            && !send_ops.contains("fn commit_send_after_emit("),
        "send progress/decision/dispatch preflight must be built into one resident commit plan before transport publication"
    );
    assert!(
        send_ops.contains("fn validate_empty_local_control_payload(")
            && send_ops.contains(
                "Self::validate_empty_local_control_payload(descriptor, payload, scratch)?"
            )
            && send_ops.contains("fn validate_explicit_wire_control_length(encoded_len: usize)")
            && send_ops.contains("Self::validate_explicit_wire_control_length(encoded_len)?")
            && send_ops.contains("descriptor.encode_payload(data, scratch)?")
            && send_ops.contains("if encoded_len != CAP_TOKEN_LEN")
            && !send_ops.contains("stage_auto_control_request")
            && !send_ops.contains("encoded_auto_control_request")
            && !send_ops.contains("fn validate_explicit_wire_control_payload(")
            && !send_ops.contains("GenericCapToken::<()>::from_raw_bytes(bytes)"),
        "send staging must use descriptor-backed empty local-control payload validation, length-only explicit wire staging, and no stale auto-mint/header validation branches"
    );

    let runtime_types = runtime_types_source();
    let send_descriptor_terminal = read("src/endpoint/kernel/core/send_descriptor_terminal.rs");
    let send_descriptor_publication =
        read("src/endpoint/kernel/core/send_descriptor_publication.rs");
    for required in [
        "descriptor: SendDescriptorTerminal<'rv>",
        "CommitDelta",
        "event_label",
        "LoopCommitRow",
        "pub(crate) fn into_ticket(self)",
        "pub(crate) fn publish(self)",
        "SendProgressCommitPlan",
        "commit_plan: Option<",
    ] {
        assert!(
            (runtime_types.contains(required)
                || send_descriptor_terminal.contains(required)
                || send_descriptor_publication.contains(required)),
            "send commit planning must carry terminal publish/rollback proofs, not replay keys: {required}"
        );
    }
    let terminal_start = send_descriptor_terminal
        .find("pub(crate) struct SendDescriptorTerminal<'rv>")
        .expect("SendDescriptorTerminal must exist");
    let terminal_body = &send_descriptor_terminal[terminal_start..];
    let publication_start = send_descriptor_publication
        .find("pub(crate) struct SendDescriptorPublication<'rv>")
        .expect("SendDescriptorPublication must exist");
    let publication_body = &send_descriptor_publication[publication_start..];
    assert!(
        terminal_body.contains("ticket: DescriptorTerminal")
            && terminal_body.contains("fn into_ticket(self)")
            && !terminal_body.contains("DescriptorPublicationAuthority")
            && !terminal_body.contains("fn publish(self)")
            && !terminal_body.contains("fn rollback(self)")
            && publication_body.contains("publisher: DescriptorPublicationAuthority<'rv>")
            && publication_body.contains("terminal: SendDescriptorTerminal<'rv>")
            && publication_body.contains("_permit: PostKernelDescriptorPermit<'rv>")
            && publication_body.contains("PostKernelDescriptorPermit::new()")
            && publication_body.contains("publisher.publish(_permit, ticket)")
            && !publication_body.contains("pub(crate) const fn new() -> Self")
            && !publication_body.contains("publisher.rollback"),
        "endpoint-resident send descriptor terminal must be ticket-only; publication authority and its unforgeable permit belong only to the post-kernel SendDescriptorPublication permit"
    );
    for forbidden in [
        "preview_cursor_index: Option<StateIndex>",
        "dispatch: Option<DescriptorDispatch>",
    ] {
        assert!(
            !terminal_body.contains(forbidden),
            "send commit proof must not keep replay-only keys: {forbidden}"
        );
    }
    let command_types = read("src/control/cluster/core/command_types.rs")
        + &read(
            "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal.rs",
        )
        + &read(
            "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/topology.rs",
        )
        + &read(
            "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/lane_effect.rs",
        )
        + &read(
            "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_terminal/publisher.rs",
        );
    let descriptor_controls = read("src/control/cluster/core/descriptor_controls.rs");
    let prepared_send_publication_owner =
        read("src/control/cluster/core/descriptor_controls/prepared_send.rs")
            + &read(
                "src/control/cluster/core/descriptor_controls/prepared_send/descriptor_effects.rs",
            );
    let prepared_effects = read("src/rendezvous/core/prepared_effects.rs");
    let snapshot_table = read("src/rendezvous/tables/snapshot.rs")
        + &read("src/rendezvous/tables/snapshot/reservation.rs");
    let lane_effects = read("src/rendezvous/core/lane_lifecycle/prepared_effects.rs")
        + &read("src/rendezvous/core/topology_process.rs");
    assert!(
        command_types.contains("pub(crate) struct DescriptorTerminal {")
            && command_types.contains("pub(crate) struct DescriptorPublicationAuthority")
            && command_types.contains("ops: &'static DescriptorPublicationAuthorityOps")
            && command_types.contains("_borrow: PhantomData<&'cfg ()>")
            && command_types.contains("struct DescriptorPublicationAuthorityOps")
            && command_types.contains("publish: unsafe fn(*const (), DescriptorTerminal)")
            && command_types.contains("_permit: PostKernelDescriptorPermit<'_>")
            && send_descriptor_publication.contains("pub(crate) struct PostKernelDescriptorPermit")
            && send_descriptor_publication
                .contains("_borrow: core::marker::PhantomData<&'permit mut ()>")
            && send_descriptor_publication.contains("const fn new() -> Self")
            && !send_descriptor_publication.contains("pub(crate) const fn new() -> Self")
            && !command_types.contains(
                "#[derive(Clone, Copy)]\npub(crate) struct DescriptorPublicationAuthority"
            )
            && !command_types.contains("rollback: unsafe fn(*const (), DescriptorTerminal)")
            && command_types.contains("ReservedTopology(")
            && command_types.contains("DescriptorEffectTerminal(")
            && command_types.contains("pub(super) enum DescriptorTerminalCase")
            && command_types.contains("pub(super) enum ReservedTopologyTerminal")
            && command_types.contains("pub(super) struct ReservedTopologyCommitPublication")
            && command_types.contains("pub(super) enum DescriptorEffectTerminal")
            && command_types.contains("pub(super) struct PreparedDescriptorEffect<Proof>")
            && command_types
                .contains("AbortBegin(PreparedDescriptorEffect<PreparedAbortBeginEffect>)")
            && command_types.contains("TxAbort(PreparedDescriptorEffect<PreparedTxAbortEffect>)")
            && prepared_send_publication_owner.contains("fn publish_descriptor_effect_terminal(")
            && prepared_send_publication_owner
                .contains("fn rollback_descriptor_effect_terminal_in_core(")
            && prepared_send_publication_owner
                .contains("publish_prepared_abort_begin_effect(proof)")
            && prepared_send_publication_owner.contains("publish_prepared_tx_abort_effect(proof)")
            && prepared_send_publication_owner.contains("rollback_prepared_tx_abort_effect(proof)")
            && prepared_effects.contains("reservation: PreparedSnapshotFinalization")
            && prepared_effects.contains("reservation: PreparedSnapshotRecord")
            && snapshot_table.contains("pub(crate) fn reserve_record(")
            && snapshot_table.contains("pub(crate) fn publish_record_reserved(")
            && snapshot_table.contains(") -> PublishedSnapshotRecord")
            && snapshot_table.contains("pub(crate) fn rollback_record_reserved(")
            && snapshot_table.contains("pub(crate) fn reserve_finalization(")
            && snapshot_table.contains("pub(crate) fn publish_finalization_reserved(")
            && snapshot_table.contains(") -> PublishedSnapshotFinalization")
            && snapshot_table.contains("pub(crate) fn rollback_finalization_reserved(")
            && lane_effects.contains("ensure_associated_session_lane(sid, lane)")
            && !lane_effects.contains("record_snapshot(")
            && !lane_effects.contains("mark_committed(")
            && !lane_effects.contains("mark_restored("),
        "descriptor terminal must split reserved topology proof from prepared descriptor-effect proofs, and descriptor effects must consume snapshot reservations through publish/rollback"
    );
    let effect_body = |function: &str| {
        let start = lane_effects
            .find(function)
            .unwrap_or_else(|| panic!("{function} must exist"));
        let rest = &lane_effects[start..];
        let end = rest[function.len()..]
            .find("\n    #[inline]")
            .map(|offset| function.len() + offset)
            .unwrap_or(rest.len());
        &rest[..end]
    };
    for (function, consume, side_effect) in [
        (
            "fn publish_prepared_state_snapshot_effect",
            "publish_record_reserved(proof.into_reservation())",
            "discard_released_lane_entries(lane)",
        ),
        (
            "fn publish_prepared_tx_commit_effect",
            "publish_finalization_reserved(proof.into_reservation())",
            "discard_released_lane_entries(lane)",
        ),
        (
            "fn publish_prepared_state_restore_effect",
            "publish_finalization_reserved(proof.into_reservation())",
            "self.r#gen.publish_prepared(lane, generation)",
        ),
        (
            "fn publish_prepared_tx_abort_effect",
            "publish_finalization_reserved(proof.into_reservation())",
            "self.r#gen.publish_prepared(lane, generation)",
        ),
    ] {
        let body = effect_body(function);
        let consume_at = body
            .find(consume)
            .unwrap_or_else(|| panic!("{function} must consume its prepared reservation"));
        let side_effect_at = body
            .find(side_effect)
            .unwrap_or_else(|| panic!("{function} must contain its terminal side effect"));
        assert!(
            consume_at < side_effect_at,
            "{function} must consume its prepared reservation before terminal side effects"
        );
    }
    for forbidden in [
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedAbortBeginEffect",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedAbortAckEffect",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedStateSnapshotEffect",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedStateRestoreEffect",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedTxCommitEffect",
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct PreparedTxAbortEffect",
    ] {
        assert!(
            !prepared_effects.contains(forbidden),
            "prepared descriptor-effect proof must be affine, not Clone/Copy: {forbidden}"
        );
    }
    for forbidden in [
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(crate) struct DescriptorTerminal",
        "pub(crate) enum DescriptorTerminal {",
        "DescriptorTerminalKind",
        "DescriptorEffectEvidence",
        "op: ControlOp",
        "fn op(&self)",
        "pub(super) enum DescriptorEffect {",
        "reserved_topology(",
        "lane_effect_evidence(",
        "fn kind(&self)",
        "publish_send_descriptor_public_endpoint",
        "terminal: unsafe fn(*const (), DescriptorTerminal, bool)",
        "publish: bool",
    ] {
        assert!(
            !command_types.contains(forbidden),
            "descriptor terminal must not keep a kind-tag, cloneable compatibility carrier, or boolean terminal dispatcher: {forbidden}"
        );
    }
    assert!(
        !descriptor_controls.contains("mod descriptor_terminal;")
            && !descriptor_controls.contains("mod prepared_topology_commit;")
            && prepared_send_publication_owner.contains("mod descriptor_terminal;")
            && prepared_send_publication_owner.contains("mod topology_commit_rollback;")
            && command_types.contains("pub(super) fn topology_begin")
            && command_types.contains("pub(super) fn topology_ack")
            && command_types.contains("pub(super) fn commit_topology")
            && command_types.contains("pub(super) const fn abort_begin")
            && command_types.contains("pub(super) const fn abort_ack")
            && command_types.contains("pub(super) const fn state_snapshot")
            && command_types.contains("pub(super) const fn state_restore")
            && command_types.contains("pub(super) const fn tx_commit")
            && command_types.contains("pub(super) const fn tx_abort")
            && !command_types.contains("pub(in crate::control::cluster::core) const fn topology_")
            && !command_types.contains("pub(in crate::control::cluster::core) const fn abort_")
            && !command_types.contains("pub(in crate::control::cluster::core) const fn state_")
            && !command_types.contains("pub(in crate::control::cluster::core) const fn tx_")
            && !command_types.contains(
                "pub(in crate::control::cluster::core::descriptor_controls) const fn topology_",
            )
            && !command_types.contains(
                "pub(in crate::control::cluster::core::descriptor_controls) const fn abort_",
            )
            && !command_types.contains(
                "pub(in crate::control::cluster::core::descriptor_controls) const fn state_",
            )
            && !command_types.contains(
                "pub(in crate::control::cluster::core::descriptor_controls) const fn tx_",
            )
            && !command_types.contains("pub(in crate::control::cluster::core::descriptor_controls) enum DescriptorTerminalCase")
            && !command_types.contains("pub(in crate::control::cluster::core::descriptor_controls) enum ReservedTopologyTerminal")
            && !command_types.contains("pub(in crate::control::cluster::core::descriptor_controls) struct ReservedTopology")
            && !command_types.contains("pub(in crate::control::cluster::core::descriptor_controls) struct DescriptorEffectTerminal")
            && !command_types.contains("pub(in crate::control::cluster::core::descriptor_controls) fn into_")
            && prepared_send_publication_owner.contains("DescriptorTerminal::topology_begin")
            && prepared_send_publication_owner.contains("DescriptorTerminal::topology_ack")
            && prepared_send_publication_owner.contains("DescriptorTerminal::commit_topology"),
        "descriptor terminal constructors and proof decomposition must be nested under the descriptor prepare owner, not guarded by source-wide semantic scanning"
    );
    let prepared_send = read("src/control/cluster/core/descriptor_controls/prepared_send.rs");
    let descriptor_controls_owner = read("src/control/cluster/core/descriptor_controls.rs");
    let cluster_storage = format!(
        "{}\n{}",
        read("src/control/cluster/core/cluster_storage.rs"),
        read("src/control/cluster/core/cluster_storage/begin_capacity.rs")
    );
    let topology_state = read("src/control/cluster/core/topology_state.rs");
    let local_topology = read("src/rendezvous/core/local_topology.rs");
    let topology_process = read("src/rendezvous/core/topology_process.rs");
    let capacity = read("src/rendezvous/core/storage_layout/capacity.rs");
    let prepared_topology_commit = read(
        "src/control/cluster/core/descriptor_controls/prepared_send/topology_commit_rollback.rs",
    );
    let prepared_commit = format!("{prepared_send}\n{prepared_topology_commit}");
    let local_prepared_commit = read("src/rendezvous/core/local_topology/prepared_commit.rs");
    let local_commit_reservation = format!(
        "{}\n{}",
        read("src/rendezvous/topology/commit_reservation.rs"),
        read("src/rendezvous/topology/commit_reservation/destination.rs")
    );
    let begin_prepare_start = prepared_send
        .find("fn prepare_topology_begin_descriptor_commit")
        .expect("TopologyBegin prepare owner must exist");
    let begin_prepare_body = &prepared_send[begin_prepare_start
        ..prepared_send
            .find("fn prepare_topology_ack_descriptor_commit")
            .expect("TopologyAck prepare owner must exist")];
    let begin_distributed_preflight = begin_prepare_body
        .find("preflight_begin(sid, operands)")
        .expect("TopologyBegin must preflight distributed replay and owner before storage ensure");
    let begin_local_preflight = begin_prepare_body
        .find("preflight_topology_begin_from_intent(")
        .expect("TopologyBegin must preflight local topology before storage ensure");
    let begin_storage_ensure = begin_prepare_body
        .find("ensure_local_topology_storage_in_core(")
        .expect("TopologyBegin must materialize local storage after preflight");
    let begin_distributed_reserve = begin_prepare_body
        .find("reserve_distributed_topology_begin_capacity(")
        .expect(
            "TopologyBegin must reserve distributed begin capacity before local materialization",
        );
    let begin_distributed_rollback = begin_prepare_body
        .find("rollback_distributed_topology_begin_capacity(")
        .expect("TopologyBegin must release reserved distributed begin capacity on local failure");
    let begin_distributed_publish = begin_prepare_body
        .find("publish_distributed_topology_begin(")
        .expect("TopologyBegin must publish distributed begin only after local proof");
    let begin_local_prepare = begin_prepare_body
        .find("prepare_topology_begin_from_intent(")
        .expect("TopologyBegin must build the local prepared topology proof");
    let begin_ticket_ready = begin_distributed_reserve
        + begin_prepare_body[begin_distributed_reserve..]
            .find(")?;")
            .expect("TopologyBegin reservation must bind a ticket before cleanup-sensitive work")
        + 3;
    let begin_after_ticket = &begin_prepare_body[begin_ticket_ready..begin_distributed_publish];
    let begin_after_publish = &begin_prepare_body[begin_distributed_publish..];
    assert!(
        begin_distributed_preflight < begin_local_preflight
            && begin_local_preflight < begin_distributed_reserve
            && begin_distributed_reserve < begin_storage_ensure
            && begin_distributed_reserve < begin_distributed_rollback
            && begin_storage_ensure < begin_local_prepare
            && begin_local_prepare < begin_distributed_publish
            && !begin_after_ticket.contains("?")
            && !begin_after_publish.contains("?")
            && !begin_prepare_body.contains("reserve_begin(")
            && !begin_prepare_body.contains("publish_distributed_topology_capacity("),
        "TopologyBegin prepare must preflight distributed/local state, release reserved begin capacity on local failure, and have no fallible step after distributed begin publication"
    );
    assert!(
        cluster_storage.contains("reserve_distributed_topology_begin_capacity(")
            && cluster_storage.contains("publish_distributed_topology_begin(")
            && cluster_storage.contains("enum ReservedDistributedTopologyBeginCapacity")
            && cluster_storage.contains("ReservedDistributedTopologyBeginCapacity::Inline")
            && cluster_storage.contains("ReservedDistributedTopologyBeginCapacity::External")
            && cluster_storage.contains("owner: RendezvousOwnerProof")
            && cluster_storage.contains("get_mut_by_proof(owner)")
            && cluster_storage.contains("owner: RendezvousOwnerProof,")
            && prepared_send.contains("reserve_distributed_topology_begin_capacity(sid, operands, owner)")
            && prepared_send.contains("rollback_distributed_topology_begin_capacity(distributed_begin_capacity)")
            && prepared_send.contains("publish_distributed_topology_begin(distributed_begin_capacity, sid, operands)")
            && cluster_storage.contains(".reserve_preflighted_begin(sid, operands)")
            && topology_state.contains("fn begin_capacity_reservation_layout(")
            && topology_state.contains("fn preflight_begin(")
            && topology_state.contains("fn preflight_abort(")
            && topology_state.contains("fn commit_preflighted_abort(")
            && topology_state.contains("Result<(TopologyOperands, DistributedPhaseKind), CpError>")
            && topology_state.contains("DistributedPhaseKind::CommitReserved")
            && topology_state.contains("TopologyError::InvalidState")
            && descriptor_controls_owner.contains("preflight_abort(sid, src_rv)?")
            && descriptor_controls_owner.contains("TopologySessionState::SourcePending")
            && descriptor_controls_owner.contains("TopologySessionState::DestinationPending")
            && descriptor_controls_owner.contains("commit_preflighted_abort(sid, operands.src_rv, operands, phase)")
            && descriptor_controls_owner.contains(".get_mut_by_proof(src_owner)")
            && descriptor_controls_owner.contains("distributed topology abort missing source local pending state")
            && descriptor_controls_owner.contains("distributed topology abort missing destination local pending state")
            && prepared_send.contains("owner: RendezvousOwnerProof")
            && prepared_send.contains("ensure_local_topology_storage_in_core(")
            && prepared_send.contains("rv.rollback_prepared_topology_begin(sid);")
            && prepared_send.contains("rv.rollback_prepared_destination_topology_ack(destination);")
            && topology_process.contains("fn rollback_prepared_topology_begin(")
            && topology_process.contains("prepared topology begin rollback missing local pending state")
            && topology_process.contains("prepared destination topology ack rollback missing local pending state")
            && !cluster_storage.contains("reserve_distributed_topology_capacity(")
            && !cluster_storage.contains("publish_distributed_topology_capacity(")
            && !cluster_storage.contains("Option<ReservedDistributedTopologyBeginCapacity>")
            && !cluster_storage.contains("reservation.owner")
            && !cluster_storage.contains("get_mut(&reservation")
            && !cluster_storage.contains("owner_proof(")
            && !prepared_send.contains("distributed_begin_capacity.take()")
            && !prepared_send.contains(".get_mut(&operands.")
            && !prepared_send.contains(".get(&operands.")
            && !prepared_send.contains(".get_mut(&target)")
            && !prepared_send.contains(".get(&target)")
            && !prepared_send.contains(".abort_topology_state(sid);")
            && !topology_state.contains("fn abort_preflighted(")
            && !topology_state.contains("fn capacity_reservation_layout(")
            && !topology_state.contains("pub(crate) fn abort(")
            && !topology_state.contains("fn reserve_begin(")
            && !descriptor_controls_owner.contains(".or_else(|| core.topology_state.get(sid))")
            && !descriptor_controls_owner.contains("local_error")
            && !descriptor_controls_owner.contains(".abort_topology_state(sid);")
            && !local_topology.contains("check_and_update(lane, Generation::ZERO)")
            && !topology_process.contains("abort_topology_state(&self, sid: SessionId) -> Result")
            && !topology_process.contains("restore_topology_generation(\n        &self,\n        lane: Lane,\n        previous_generation: Option<Generation>,\n    ) -> Result")
            && !topology_process.contains("rollback_prepared_destination_topology_ack(\n        &self,\n        proof: PreparedDestinationTopologyAck,\n    ) -> bool")
            && !topology_process.contains("check_and_update(lane, Generation::ZERO)")
            && capacity.contains("enum LaneStorageShape")
            && capacity.contains("struct LaneStorageReservation")
            && capacity.contains("release_lane_storage_reservation(")
            && !capacity.contains("include_topology")
            && !capacity.contains("assoc_storage")
            && !capacity.contains("snapshot_storage")
            && !capacity.contains("policy_storage")
            && !capacity.contains("topology_storage")
            && !capacity.contains("prepare_topology_control_scope")
            && !capacity.contains("ensure_core_lane_storage(&mut self)")
            && !capacity.contains("core_growth && old_policy_bound"),
        "TopologyBegin and lane-storage growth must not retain generic capacity publish APIs, local generation mutation before prepared state, boolean storage modes, or test-only owner helpers"
    );
    let commit_prepare_start = prepared_send
        .find("fn prepare_topology_commit_descriptor_commit")
        .expect("TopologyCommit prepare owner must exist");
    let commit_prepare_body = &prepared_send[commit_prepare_start
        ..prepared_send
            .find("pub(crate) fn rollback_descriptor_terminal")
            .expect("TopologyCommit prepare body must be bounded by rollback owner")];
    let commit_distributed_preflight = commit_prepare_body
        .find("preflight_commit(")
        .expect("TopologyCommit must preflight distributed state");
    let commit_source_preflight = commit_prepare_body
        .find("validate_topology_commit_operands(")
        .expect("TopologyCommit must preflight source-local state");
    let commit_destination_preflight = commit_prepare_body
        .find("preflight_destination_topology_commit(")
        .expect("TopologyCommit must preflight destination-local state");
    let commit_storage_require = commit_prepare_body
        .find("require_local_topology_storage_in_core(")
        .expect("TopologyCommit must require pre-existing local storage after preflight");
    let commit_source_reserve = commit_prepare_body
        .find("reserve_source_topology_commit(")
        .expect("TopologyCommit must reserve source proof after storage requirement");
    assert!(
        commit_distributed_preflight < commit_source_preflight
            && commit_source_preflight < commit_destination_preflight
            && commit_destination_preflight < commit_storage_require
            && commit_storage_require < commit_source_reserve
            && !commit_prepare_body.contains("ensure_local_topology_storage_in_core(")
            && !commit_prepare_body.contains("if let Some(rv) = core.locals.get_mut"),
        "TopologyCommit prepare must complete distributed/source/destination preflight and must not materialize storage before proof reservation"
    );
    assert!(
        local_commit_reservation.contains(
            "pub(crate) struct PreparedSourceTopologyCommit {\n    slot: u8,\n    previous_generation: Option<Generation>,\n    target: Generation,"
        ) && local_prepared_commit
            .contains("assert_eq!(self.r#gen.last(lane), ticket.previous_generation());"),
        "source topology commit proof must bind the source lane previous generation, matching destination proof strength"
    );
    for required in [
        "publish_prepared_begin(",
        "publish_prepared_ack(",
        "publish_prepared_commit(",
        "reserve_source_topology_commit(",
        "reserve_destination_topology_commit(",
        "assert_prepared_source_topology_commit(",
        "assert_prepared_destination_topology_commit(",
        "assert_prepared_commit(&distributed)",
        "publish_prepared_source_topology_commit(",
        "publish_prepared_destination_topology_commit(",
        "rollback_source_topology_commit_reservation(",
        "rollback_destination_topology_commit_reservation(",
    ] {
        assert!(
            prepared_commit.contains(required),
            "TopologyCommit publication must mint, consume, or rollback owner proofs: {required}"
        );
    }
    for forbidden in [
        "publish_begin_reserved(",
        "publish_ack_reserved(",
        "rollback_ack_reserved(",
        "get_from(sid, src_rv)",
        "preflight_commit_reserved(",
        "ensure_commit_reserved(",
        "publish_commit_reserved(",
        "consume_prepared_commit(",
    ] {
        assert!(
            !prepared_send.contains(forbidden),
            "prepared topology publish must not replay reserved-phase validation: {forbidden}"
        );
    }
    let destination_publish = prepared_send
        .find(".publish_prepared_destination_topology_commit(destination, meta.dst_lane())")
        .expect("destination prepared topology proof must be consumed");
    let source_publish = prepared_send
        .find("(&mut *src_ptr).publish_prepared_source_topology_commit(")
        .expect("source prepared topology proof must be consumed");
    let distributed_consume = prepared_send
        .find("publish_prepared_commit(distributed)")
        .expect("distributed prepared topology proof must be terminally consumed");
    let distributed_assert = prepared_send
        .find("assert_prepared_commit(&distributed)")
        .expect("distributed commit invariant must be asserted before terminal consumption");
    let destination_assert = prepared_send
        .find("assert_prepared_destination_topology_commit(")
        .expect("destination commit invariant must be asserted before terminal consumption");
    let source_assert = prepared_send
        .find("assert_prepared_source_topology_commit(")
        .expect("source commit invariant must be asserted before terminal consumption");
    let distributed_success = prepared_send
        .find("Distributed commit proof is consumed;")
        .expect("distributed prepared topology success boundary must be explicit");
    assert!(
        distributed_assert < destination_assert
            && destination_assert < source_assert
            && source_assert < distributed_consume
            && distributed_consume < distributed_success
            && distributed_success < destination_publish
            && destination_publish < source_publish,
        "TopologyCommit publication must assert all owner invariants before distributed proof consumption, then publish local proofs without fallible branches"
    );
    let pre_distributed_window = &prepared_send[..distributed_consume];
    assert!(
        !pre_distributed_window.contains("publish_prepared_destination_topology_commit(")
            && !pre_distributed_window.contains("publish_prepared_source_topology_commit("),
        "TopologyCommit publication must not mutate local topology before distributed terminal proof consumption"
    );
    let post_distributed_window = &prepared_send[distributed_success..destination_publish];
    assert!(
        !post_distributed_window.contains("return;")
            && !post_distributed_window.contains("get_pair_mut(")
            && !post_distributed_window.contains("owners disappeared"),
        "TopologyCommit publication must not keep owner lookup or early-return paths after distributed terminal proof consumption"
    );
    let post_destination_window = &prepared_send[destination_publish..source_publish];
    assert!(
        !post_destination_window.contains("return;")
            && !post_destination_window.contains("prepared source rendezvous disappeared")
            && !post_destination_window.contains("prepared destination rendezvous disappeared"),
        "TopologyCommit publication must not keep an early-return path after consuming a local prepared proof"
    );
    for forbidden in [
        "preflight_prepared_topology_commit(",
        "prepared_topology_commit_ready(",
        "prepared_topology_commit_owners_present(",
        "ensure_reserved_source_topology_commit(",
        "ensure_reserved_destination_topology_commit(",
        "consume_reserved_source_topology_commit(",
        "consume_reserved_destination_topology_commit(",
        "publish_reserved_source_topology_commit(",
        "publish_reserved_destination_topology_commit(",
        "publish_commit_reserved(",
        "topology_commit(sid, source_lane).is_err()",
    ] {
        assert!(
            !prepared_commit.contains(forbidden),
            "TopologyCommit post-transport publish must not retain fallible local publication branches: {forbidden}"
        );
    }
    for publish in [
        "publish_prepared_destination_topology_commit",
        "publish_prepared_source_topology_commit",
    ] {
        let start = local_prepared_commit
            .find(&format!("fn {publish}("))
            .unwrap_or_else(|| panic!("missing local topology proof publisher: {publish}"));
        let body = &local_prepared_commit[start..];
        let body = &body[..body.find("\n    pub(crate) fn ").unwrap_or(body.len())];
        for forbidden in ["assert!", "assert_eq!", ".is_err()", ".unwrap()"] {
            assert!(
                !body.contains(forbidden),
                "local prepared topology commit publish must not hide fail-closed branches behind proof consumption: {publish}: {forbidden}"
            );
        }
    }
    for consumer in [
        "rollback_source_commit_reserved",
        "rollback_destination_commit_reserved",
        "assert_source_commit_reserved",
        "assert_destination_commit_reserved",
        "clear_prepared_source_commit_unchecked",
        "finalize_prepared_destination_commit_unchecked",
    ] {
        let start = local_commit_reservation
            .find(&format!("fn {consumer}("))
            .unwrap_or_else(|| panic!("missing local topology proof consumer: {consumer}"));
        let body = &local_commit_reservation[start..];
        let body = &body[..body
            .find("\n    pub(in crate::rendezvous) fn ")
            .unwrap_or(body.len())];
        let forbidden_debug_guard = ["debug", "_assert"].concat();
        assert!(
            !body.contains(&forbidden_debug_guard) && !body.contains("return;"),
            "local topology proof consumer must assert invariants in release, not debug-return: {consumer}"
        );
    }
    for forbidden in ["SendState::Committing", "Committing {"] {
        assert!(
            !runtime_types.contains(forbidden) && !send_ops.contains(forbidden),
            "send state must not keep an extra post-transport staging variant: {forbidden}"
        );
    }
    assert!(
        send_ops.contains("EndpointTx policy audit is an attempt-side replay tuple")
            && send_ops.contains("ENDPOINT_SEND / ENDPOINT_CONTROL event is emitted only from the")
            && send_ops
                .find("self.emit_endpoint_policy_audit(")
                .expect("EndpointTx policy audit must exist")
                < send_ops
                    .find("self.build_send_commit_plan(")
                    .expect("send commit plan must exist"),
        "EndpointTx audit must be documented and ordered as an attempt audit, not confused with the commit event"
    );
}
