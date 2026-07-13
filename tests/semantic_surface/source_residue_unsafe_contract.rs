use super::common::*;

#[test]
fn unsafe_contract_gate_covers_receive_frame_receipt_owner() {
    let gate = read(".github/scripts/check_unsafe_contract_hygiene.sh");
    let receipt = read("src/rendezvous/recv_frame_receipt.rs");
    let port_recv = read("src/rendezvous/port/recv_frame.rs");

    assert!(
        gate.contains("cat src/rendezvous/recv_frame_receipt.rs")
            && gate.contains("RecvFrameReceiptPhase::Outstanding")
            && receipt.contains("enum RecvFrameReceiptPhase")
            && receipt.contains("owner: Option<RecvFrameReceiptOwner>")
            && receipt.contains("None => crate::invariant()")
            && !receipt.contains("if self.state.is_null()")
            && !receipt.contains("if self.outstanding.replace(true)")
            && port_recv.contains("impl Drop for ReceivedFrameCore"),
        "unsafe contract gate must scan both the receive-frame value owner and its receipt authority owner"
    );
}

#[test]
fn external_callback_reentry_revalidates_published_endpoint_generation() {
    let registry = read("src/session/lease/core/registry_ops.rs");
    let cluster = read("src/session/cluster/core/session_cluster_ops.rs");
    let port = read("src/rendezvous/core/access_port.rs");
    let resolver_offer = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    let resolver_send = read("src/endpoint/kernel/core/decision_resolver/impls/send.rs");
    let transport_send = read("src/endpoint/kernel/core/send_ops.rs");
    let lane_port = read("src/endpoint/kernel/lane_port.rs");
    let transport_recv = read("src/endpoint/kernel/observe.rs");
    let recv_commit = read("src/endpoint/kernel/recv_commit_plan.rs");
    let offer_ingress = read("src/endpoint/kernel/offer/ingress.rs");
    let requeue_regression = read("src/endpoint/kernel/offer/requeue_callback_tests.rs");
    let regression =
        read("tests/miri_runtime_owner.rs") + &read("tests/miri_runtime_owner/callback_reentry.rs");
    let miri_gate = read(".github/scripts/check_miri.sh");
    let wide_roll_topology = regression
        .split("fn wide_roll_shared_site_program")
        .nth(1)
        .and_then(|tail| tail.split("fn narrow_roll_shared_site_program").next())
        .expect("wide-roll Miri topology fixture");
    let narrow_roll_topology = regression
        .split("fn narrow_roll_shared_site_program")
        .nth(1)
        .and_then(|tail| tail.split("#[repr(align(16))]").next())
        .expect("narrow-roll Miri topology fixture");
    let wide_rolls_whole_tail = wide_roll_topology
        .lines()
        .any(|line| line.trim() == ".roll(),");
    let narrow_rolls_first_tail_atom = narrow_roll_topology.lines().any(|line| {
        let line = line.trim();
        line.contains("g::send::<") && line.ends_with(".roll(),")
    });

    assert!(
        registry.contains("fn published_endpoint_owner(")
            && registry.contains("rendezvous.endpoint_lease_storage(slot, generation)?")
            && cluster.contains("published_endpoint_owner(")
            && cluster.contains("slot, generation")
            && port.contains(".transport\n            .open(")
            && port.contains("if self.session_fault(sid).is_some()")
            && regression
                .contains("transport_open_reentry_cannot_publish_endpoint_into_poisoned_session")
            && regression
                .contains("resolver_callback_reentry_cannot_materialize_branch_after_peer_drop")
            && regression.contains("resolver_callback_reentry_cannot_prepare_send_after_peer_drop")
            && regression.contains("transport_send_callback_reentry_cannot_commit_after_peer_drop")
            && regression
                .contains("transport_cancel_callback_reentry_consumes_cleanup_authority_once")
            && regression
                .contains("transport_recv_callback_reentry_cannot_commit_frame_after_peer_drop")
            && regression
                .contains("payload_validation_callback_reentry_cannot_commit_after_peer_drop")
            && regression
                .contains("payload_encoding_callback_reentry_cannot_commit_after_peer_drop")
            && regression.contains("resolver_registration_keeps_distinct_program_image_identity")
            && wide_roll_topology.matches(".roll()").count() == 1
            && narrow_roll_topology.matches(".roll()").count() == 1
            && wide_rolls_whole_tail
            && narrow_rolls_first_tail_atom
            && regression
                .contains("failed_resolver_growth_preserves_existing_registration_and_dispatch")
            && regression.contains("Cluster(exhausted resolver)")
            && resolver_offer.contains("let resolver_result =")
            && resolver_offer.contains("cluster.resolve_dynamic_resolver(")
            && resolver_offer.contains("self.port().seal_session_membership()")
            && resolver_offer.contains("return Err(RecvError::SessionFault(kind))")
            && resolver_send.contains("let resolver_result =")
            && resolver_send.contains("self.cursor.program_ref(),")
            && resolver_send.contains("return Err(SendError::SessionFault(kind))")
            && transport_send.contains("let transport_poll = lane_port::poll_send_outgoing")
            && transport_send.contains("cancel_send_outgoing(&mut pending.transport, port)")
            && lane_port.contains("pending.state = SendIoState::Unpolled;\n    unsafe {")
            && transport_recv
                .matches("if let Some(kind) = self.session_fault()")
                .count()
                >= 2
            && transport_recv
                .matches("frame.discard_uncommitted()")
                .count()
                >= 2
            && recv_commit.contains("let validation = plan.payload.validate(validate)")
            && recv_commit.contains("return Err(RecvError::SessionFault(kind))")
            && offer_ingress.contains("let requeue = payload.requeue_on(port)")
            && offer_ingress.contains("return Err(RecvError::SessionFault(kind))")
            && requeue_regression
                .contains("transport_requeue_callback_reentry_revalidates_generation")
            && miri_gate.contains("--test miri_runtime_owner"),
        "external callback reentry must revalidate the published endpoint generation before attach, offer, or send can continue"
    );
}

#[test]
fn dynamic_resolver_seals_local_membership_before_external_evaluation() {
    let lease_state = read("src/rendezvous/core.rs");
    let access_state = read("src/rendezvous/core/access_state.rs");
    let lease_owner = read("src/rendezvous/core/storage_layout/capacity/endpoint_lease.rs");
    let registry = read("src/session/lease/core/registry_ops.rs");
    let resolver_offer = read("src/endpoint/kernel/core/decision_resolver/impls/select.rs");
    let resolver_send = read("src/endpoint/kernel/core/decision_resolver/impls/send.rs");
    let carrier = read("src/endpoint/carrier.rs");
    let regression = read("tests/dynamic_route_scope_resolver.rs")
        + &read("tests/miri_runtime_owner/callback_reentry.rs");
    let miri_gate = read(".github/scripts/check_miri.sh");

    let offer_seal = resolver_offer
        .find("self.port().seal_session_membership()")
        .expect("offer resolver membership seal");
    let offer_callback = resolver_offer
        .find("cluster.resolve_dynamic_resolver(")
        .expect("offer resolver callback");
    let send_seal = resolver_send
        .find("self.port().seal_session_membership()")
        .expect("send resolver membership seal");
    let send_callback = resolver_send
        .find("cluster.resolve_dynamic_resolver(")
        .expect("send resolver callback");
    let busy = registry
        .find("if rendezvous.access_is_busy()")
        .expect("endpoint-operation barrier rejection");
    let reject = registry
        .find("session_membership_is_sealed(sid)")
        .expect("sealed attach rejection");
    let image_read = registry
        .find("endpoint_session_program(sid)")
        .expect("program identity read");
    let operation_lease = carrier
        .find("endpoint.try_public_operation_lease()")
        .expect("public endpoint operation lease");
    let operation = carrier
        .find("f(endpoint)")
        .expect("public endpoint operation");

    assert!(lease_state.contains("MembershipSealed = 3"));
    assert!(lease_state.contains("Self::Published | Self::MembershipSealed"));
    assert!(access_state.contains("EndpointOperation = 3"));
    assert!(lease_owner.contains("fn seal_session_membership("));
    assert!(offer_seal < offer_callback);
    assert!(send_seal < send_callback);
    assert!(
        busy < image_read,
        "operation barrier must reject before raw image read"
    );
    assert!(
        reject < image_read,
        "sealed attach must reject before raw image read"
    );
    assert!(operation_lease < operation);
    assert!(
        regression.contains("dynamic_resolution_seals_runtime_local_membership_before_evaluation")
    );
    assert!(
        regression
            .contains("resolver_callback_reentrant_attach_is_rejected_before_endpoint_image_read")
    );
    assert!(miri_gate.contains("dynamic-membership-seal-owner"));
}

#[test]
fn route_waiter_callbacks_run_only_after_complete_publication() {
    let port = read("src/rendezvous/port/route_publication.rs");
    let route_table = read("src/rendezvous/tables/route_table.rs");
    let route_storage = read("src/rendezvous/tables/route_table/storage.rs");
    let send = read("src/endpoint/kernel/core/send_ops.rs");
    let offer = read("src/endpoint/kernel/offer/commit.rs");
    let offer_select = read("src/endpoint/kernel/offer/select.rs");
    let offer_select_observed = read("src/endpoint/kernel/offer/select_observed.rs");
    let offer_resolve = read("src/endpoint/kernel/offer/resolve.rs");
    let begin = port
        .split("pub(crate) fn begin_route_arm_selection(")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn observe_active_route_arm_selection(")
                .next()
        })
        .expect("route begin publication boundary");
    let observe = port
        .split("pub(crate) fn observe_active_route_arm_selection(")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(crate) fn wake_route_arm_selection_waiters(")
                .next()
        })
        .expect("route observation publication boundary");
    let send_publish = send
        .split("fn publish_send_progress_commit_plan(")
        .nth(1)
        .and_then(|tail| tail.split("fn build_send_commit_plan(").next())
        .expect("send route publication boundary");
    let offer_publish = offer
        .split("fn publish_branch_preview_commit_plan(")
        .nth(1)
        .expect("offer route publication boundary");

    assert!(!begin.contains("wake_session_waiters"));
    assert!(!observe.contains("wake_session_waiters"));
    assert!(port.contains("fn route_observed_mask(&self, peer: Option<u8>) -> u16"));
    assert!(port.contains("fn attached_session_role_mask(&self) -> u16"));
    assert!(port.contains("RouteTable::selected_local_participant_mask("));
    assert!(route_table.contains("selected_participants & attached_roles"));
    let prepared_entry = route_table
        .find("RouteEntry::EMPTY.try_begin(initial_observed, arm)")
        .expect("route entry must be prepared before publication");
    let published_entry = route_table
        .find("self.publish_slot(coord, entry)")
        .expect("prepared route entry publication");
    assert!(prepared_entry < published_entry);
    assert!(route_table.contains("RouteFrame::assign(coord, entry, head)"));
    assert!(!route_table.contains("begin_frame_entry"));
    for stale in ["lane_heads", "lane_base", "lane_slots"] {
        assert!(!route_table.contains(stale));
        assert!(!route_storage.contains(stale));
    }
    let send_route = send_publish
        .find("publish_send_route_evidence_delta(")
        .expect("send route publication");
    let send_event = send_publish
        .find("emit_send_after_transport_event(")
        .expect("send event publication");
    let send_wake = send_publish
        .find("wake_route_arm_selection_waiters(")
        .expect("send route waiter notification");
    assert!(send_route < send_event && send_event < send_wake);
    let offer_clear = offer_publish
        .rfind("clear_scope_evidence(scope_id)")
        .expect("offer evidence retirement");
    let offer_wake = offer_publish
        .rfind("wake_route_arm_selection_waiters(lane_wire)")
        .expect("offer route waiter notification");
    assert!(offer_clear < offer_wake);
    assert!(offer_publish.contains("route_token.is_poll()"));
    assert!(offer_publish.contains("self.cursor.route_scope_resolver(scope_id).is_some()"));
    assert!(offer_publish.contains("self.observe_active_route_arm_selection("));
    let framed_reject = offer_select
        .find("if let Some(observed) = carried_observation")
        .expect("unmatched framed ingress rejection");
    let lane_only = offer_select
        .find("self.select_carried_ingress_scope(carried_lane)")
        .expect("deterministic lane-only selection");
    assert!(framed_reject < lane_only);
    assert!(offer_select.contains("FrameMismatch::label_mismatch(observed)"));
    let arm_conflict = offer_select_observed
        .find("selected != observed")
        .expect("framed arm conflict");
    let cursor_realign = offer_select_observed
        .find("self.commit_cursor_realign_index(target_idx)")
        .expect("observed target cursor realignment");
    assert!(arm_conflict < cursor_realign);
    assert!(offer_select_observed.contains("return Err(RecvError::PhaseInvariant)"));
    assert!(offer_resolve.contains("preview_live_route_arm_selection_non_consuming(scope_id)"));
    assert!(offer_resolve.contains("state.ingress.take_transport()"));
    assert!(offer_resolve.contains("frame.discard_uncommitted()"));
}
