use super::common::*;

#[test]
fn public_endpoint_operations_are_drop_independent_active_leases() {
    let public_types = read("src/endpoint/kernel/core/public_types.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let public_poll = read("src/endpoint/kernel/public_poll.rs");
    let public_runtime = format!("{public_ops}\n{public_poll}");
    let endpoint_ops = read("src/endpoint/ops.rs");
    let send = read("src/endpoint/send.rs");
    let send_resolver_proof = read("src/endpoint/kernel/core/send_resolver_proof.rs");
    let futures = read("src/endpoint/futures.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let branch = read("src/endpoint/branch.rs");
    let route_preview = read("src/endpoint/kernel/core/send_preview.rs");
    let lifecycle_tests = [
        read("tests/cursor_send_recv/session_progress.rs"),
        read("tests/cursor_send_recv/session_forget_send.rs"),
        read("tests/cursor_send_recv/session_forget_recv.rs"),
        read("tests/cursor_send_recv/session_fault_cancel.rs"),
        read("tests/cursor_send_recv/session_drop_wake.rs"),
        read("tests/offer_branch_recv_evidence.rs"),
        read("tests/route_branch_send.rs"),
        read("tests/nested_route_runtime.rs"),
        read("tests/affine_progression.rs"),
    ]
    .join("\n");

    for required in [
        "pub(in crate::endpoint) enum PublicActiveOp",
        "Idle",
        "Poisoned",
        "Send",
        "Recv",
        "Offer",
        "RouteBranch",
        "BranchRecv",
        "BranchSend",
        "public_active_op: PublicActiveOp",
    ] {
        assert!(
            public_types.contains(required),
            "endpoint kernel active-operation lease evidence missing: {required}"
        );
    }
    assert!(
        public_types.contains("pub(crate) enum PublicOpLease")
            && public_types.contains("Rejected = 0")
            && public_types.contains("Held = 1")
            && public_runtime.contains("fn start_public_op(\n        &mut self,\n        op: PublicActiveOp,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains("self.public_active_op == PublicActiveOp::Idle")
            && public_runtime.contains("self.public_active_op = PublicActiveOp::Poisoned;")
            && public_runtime.contains("SessionFaultKind::ProgressInvariantViolated")
            && !public_runtime.contains("fn enter_public_op(&mut self, op: PublicActiveOp) -> bool")
            && public_runtime.contains("fn transition_public_op(\n        &mut self,\n        from: PublicActiveOp,\n        to: PublicActiveOp,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains("fn finish_public_op(&mut self, op: PublicActiveOp)")
            && public_runtime.contains("if self.public_active_op == op")
            && public_runtime.contains("self.public_active_op != PublicActiveOp::Poisoned")
            && public_runtime.contains("fn clear_public_op_if_current(&mut self, op: PublicActiveOp)")
            && public_runtime.contains("fn init_public_send_state(\n        &mut self,\n        init: &SendInit,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains("PublicActiveOp::Idle => self.start_public_op(PublicActiveOp::Send)")
            && public_runtime.contains("PublicActiveOp::RouteBranch =>")
            && public_runtime.contains(
                "self.transition_public_op(PublicActiveOp::RouteBranch, PublicActiveOp::BranchSend)"
            )
            && public_runtime.contains("self.public_active_op = PublicActiveOp::RouteBranch;")
            && public_runtime.contains("fn init_public_recv_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("let lease = self.start_public_op(PublicActiveOp::Recv);")
            && public_runtime.contains("fn init_public_offer_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("let lease = self.start_public_op(PublicActiveOp::Offer);")
            && public_runtime.contains("super::core::PublicOpLease::Rejected => return lease")
            && public_runtime.contains("if self.public_active_op != PublicActiveOp::Offer")
            && public_runtime.contains("self.public_active_op = PublicActiveOp::RouteBranch;")
            && public_runtime.contains("fn begin_public_branch_recv_state(\n        &mut self,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains("PublicActiveOp::BranchRecv")
            && public_runtime.contains("PublicActiveOp::BranchSend")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Send)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Recv)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Offer)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::BranchRecv)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::BranchSend)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::RouteBranch)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Send)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Recv)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Offer)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::BranchRecv)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::BranchSend)")
            && public_runtime.contains("if self.public_route_branch.is_some() {\n            self.clear_session_waiter();\n            self.public_op_busy_fault();")
            && public_runtime.contains("let Some(branch) = branch_recv_state.branch() else {\n            self.clear_session_waiter();\n            self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();\n            self.public_op_busy_fault();"),
        "endpoint API calls must start only from Idle, checked completion must poison on wrong active-op order, and terminal sweep must use an explicit lenient clear path"
    );
    assert!(
        route_preview.contains("PublicActiveOp::Idle")
            && route_preview.contains(
                "PublicActiveOp::RouteBranch => return self.preview_branch_send_meta(target_label)"
            )
            && route_preview.contains("self.public_op_busy_fault();")
            && route_preview.contains("return Err(SendError::PhaseInvariant);"),
        "send preview must not overwrite a forgotten active operation and must route branch send through the selected-arm token"
    );
    assert!(
        endpoint_ops.contains("fn call_handle_op(")
            && endpoint_ops.contains("fn call_lease_op(")
            && endpoint_ops.contains("fn call_send_init_op(")
            && !endpoint_ops.contains("pub(super) unsafe fn init_public_send_state")
            && endpoint_ops.contains("match self.init_public_send_state(&init)")
            && endpoint_ops.contains("kernel::PublicOpLease::Held => Ok(())")
            && endpoint_ops.contains("kernel::PublicOpLease::Rejected =>")
            && endpoint_ops.contains("crate::endpoint::SendError::PhaseInvariant"),
        "send first-poll arming must fail locally when active-lease initialization is rejected, with carrier unsafe concentrated in private helpers"
    );
    let direct_send_constructor = endpoint_ops
        .split("pub fn send<'a, 'e, M>(")
        .nth(1)
        .and_then(|tail| {
            tail.split("/// Receive the next deterministic message")
                .next()
        })
        .expect("direct send constructor must stay visible");
    let branch_send_constructor = branch
        .split("pub fn send<'a, M>(")
        .nth(1)
        .and_then(|tail| {
            tail.split("impl<'r, const ROLE: u8> Drop for Endpoint")
                .next()
        })
        .expect("route branch send constructor must stay visible");
    let send_future_poll = send
        .split("fn poll_raw(")
        .nth(1)
        .and_then(|tail| tail.split("impl<'a, 'e, 'r, const ROLE: u8> Future").next())
        .expect("send future poll must stay visible");
    assert!(
        send.contains("impl<'a, 'e, 'r, const ROLE: u8> Drop for RawSendFuture")
            && send.contains("enum SendFutureState")
            && send.contains("DirectUnarmed")
            && send.contains("Armed")
            && send.contains("ReadyError(SendError)")
            && send.contains("Done")
            && send.contains("fn arm_on_first_poll(&mut self)")
            && send.contains("endpoint.begin_public_send_state(logical_label)")
            && send.contains("pub(crate) fn ready_error(error: SendError) -> Self")
            && send.contains("payload: Option<kernel::RawSendPayload>")
            && send.contains("state: SendFutureState")
            && !send.contains("ready_error: Option")
            && !send.contains("logical_label: 0")
            && !send.contains("RawSendPayload::empty")
            && direct_send_constructor
                .contains("SendFuture::pending_direct(endpoint, logical_label, payload)")
            && !direct_send_constructor.contains("preview_send(")
            && !direct_send_constructor.contains("init_public_send_state")
            && branch_send_constructor.contains("preview_send(logical_label")
            && branch_send_constructor.contains("send_runtime_desc(\n            logical_label,")
            && branch_send_constructor.contains("init_public_send_state(&init)")
            && branch_send_constructor.contains("SendFuture::pending_armed(endpoint, payload)")
            && send_future_poll.contains("self.arm_on_first_poll()")
            && send_future_poll.contains("endpoint.poll_send(cx, self.payload.take())")
            && endpoint_ops.contains("pub fn send<'a, 'e, M>(")
            && !endpoint_ops.contains("pub fn flow")
            && !send.contains("struct Flow")
            && futures.contains("enum OfferFutureLease")
            && futures.contains("enum RecvFutureLease")
            && branch.contains("OfferFutureLease::from_public_lease")
            && futures.contains("RecvFutureLease::from_public_lease")
            && !runtime_types.contains("RecvPayloadMode")
            && !futures.contains("RecvPayloadMode")
            && !futures.contains("payload_mode")
            && branch.contains("let lease = endpoint.init_public_offer_state();")
            && branch.contains("lease: OfferFutureLease::from_public_lease(lease)")
            && branch.contains("OfferFutureLease::Rejected =>")
            && branch.contains("self.lease = OfferFutureLease::Completed;")
            && branch.contains("self.lease == OfferFutureLease::RestoreOnDrop")
            && futures.contains("RecvFutureLease::Rejected =>")
            && futures.contains("self.lease = RecvFutureLease::Completed;")
            && futures.contains("lease: crate::endpoint::kernel::PublicOpLease")
            && futures.contains("self.lease == RecvFutureLease::RestoreOnDrop")
            && futures.contains("progress: BranchRecvFutureProgress")
            && futures.contains("BranchRecvFutureProgress::Pending")
            && futures.contains("BranchRecvFutureProgress::Finished")
            && futures.contains(
                "(crate::endpoint::kernel::PublicOpLease::Held, BranchRecvFutureProgress::Pending)"
            )
            && futures.contains("impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture")
            && futures.contains("impl<'e, 'r, const ROLE: u8> Drop for RawBranchRecvFuture"),
        "Drop cleanup may remain as acquired-lease cleanup, but only futures that actually acquired the resident active lease may restore it; failed constructors must report PhaseInvariant without touching another active operation's waiter/state"
    );
    let resolver_authority = send_resolver_proof
        .split("pub(crate) enum SendResolverAuthority")
        .nth(1)
        .and_then(|tail| tail.split("impl SendResolverAuthority").next())
        .expect("send resolver authority must stay explicit");
    assert!(
        resolver_authority.contains("Direct(ResolverDecisionProofs)")
            && resolver_authority.contains("MaterializedBranch"),
        "send resolver authority must distinguish direct preview proofs from materialized route branches"
    );
    for forbidden in ["&", "Payload", "Codec", "RawSendPayload", "Wire"] {
        assert!(
            !resolver_authority.contains(forbidden),
            "send resolver authority must not carry references, payloads, or codec hooks: {forbidden}"
        );
    }
    for required in [
        "forgotten_unpolled_send_future_does_not_publish_runtime_progress",
        "forgotten_started_send_future_leaves_send_fail_closed",
        "drop_unpolled_send_future_keeps_endpoint_on_same_send_step",
        "forgotten_recv_future_leaves_endpoint_fail_closed",
        "forgotten_started_offer_future_leaves_endpoint_fail_closed",
        "forgotten_route_branch_leaves_endpoint_fail_closed",
        "forgotten_route_recv_future_leaves_endpoint_fail_closed",
        "core::mem::forget",
    ] {
        assert!(
            lifecycle_tests.contains(required),
            "mem::forget behavior must be covered by runtime regression tests: {required}"
        );
    }
}
