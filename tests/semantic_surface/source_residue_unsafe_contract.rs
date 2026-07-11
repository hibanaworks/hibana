use super::common::*;

#[test]
fn unsafe_contract_gate_covers_receive_frame_receipt_owner() {
    let gate = read(".github/scripts/check_unsafe_contract_hygiene.sh");
    let receipt = read("src/rendezvous/recv_frame_receipt.rs");
    let port_recv = read("src/rendezvous/port/recv_frame.rs");

    assert!(
        gate.contains("cat src/rendezvous/recv_frame_receipt.rs")
            && gate.contains("if self.outstanding.replace(true)")
            && receipt.contains("if self.outstanding.replace(true)")
            && receipt.contains("if !self.outstanding.get()")
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
            && regression
                .contains("failed_resolver_growth_preserves_existing_registration_and_dispatch")
            && regression.contains("Cluster(exhausted resolver)")
            && resolver_offer.contains("let resolver_result =")
            && resolver_offer.contains(
                "cluster.resolve_dynamic_resolver(rv_id, self.cursor.program_ref(), resolver)"
            )
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
