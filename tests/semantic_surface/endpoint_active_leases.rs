use super::common::*;

#[test]
fn public_endpoint_operations_are_drop_independent_active_leases() {
    let public_types = read("src/endpoint/kernel/core/public_types.rs");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    let public_poll = read("src/endpoint/kernel/public_poll.rs");
    let public_runtime = format!("{public_ops}\n{public_poll}");
    let endpoint_ops = read("src/endpoint/ops.rs");
    let flow = read("src/endpoint/flow.rs");
    let futures = read("src/endpoint/futures.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let branch = read("src/endpoint/branch.rs");
    let route_preview = read("src/endpoint/kernel/core/route_preview_flow.rs");
    let lifecycle_tests = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        read("tests/cursor_send_recv/session_progress.rs"),
        read("tests/cursor_send_recv/session_forget_flow.rs"),
        read("tests/cursor_send_recv/session_forget_send.rs"),
        read("tests/cursor_send_recv/session_forget_recv.rs"),
        read("tests/cursor_send_recv/session_fault_cancel.rs"),
        read("tests/cursor_send_recv/session_drop_wake.rs"),
        read("tests/offer_decode_receive_evidence.rs"),
        read("tests/nested_route_runtime.rs"),
        read("tests/affine_progression.rs")
    );

    for required in [
        "pub(in crate::endpoint) enum PublicActiveOp",
        "Idle",
        "Poisoned",
        "Send",
        "Recv",
        "Offer",
        "RouteBranch",
        "Decode",
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
            && public_runtime.contains("let lease = self.start_public_op(PublicActiveOp::Send);")
            && public_runtime.contains("fn init_public_recv_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("let lease = self.start_public_op(PublicActiveOp::Recv);")
            && public_runtime.contains("fn init_public_offer_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("let lease = self.start_public_op(PublicActiveOp::Offer);")
            && public_runtime.contains("super::core::PublicOpLease::Rejected => return lease")
            && public_runtime.contains("if self.public_active_op != PublicActiveOp::Offer")
            && public_runtime.contains("self.public_active_op = PublicActiveOp::RouteBranch;")
            && public_runtime.contains("fn begin_public_decode_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("self.transition_public_op(PublicActiveOp::RouteBranch, PublicActiveOp::Decode)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Send)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Recv)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Offer)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::Decode)")
            && public_runtime.contains("self.finish_public_op(PublicActiveOp::RouteBranch)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Send)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Recv)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Offer)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Decode)")
            && public_runtime.contains("if self.public_route_branch.is_some() {\n            self.clear_session_waiter();\n            self.public_op_busy_fault();")
            && public_runtime.contains("let Some(branch) = decode_state.branch() else {\n            self.clear_session_waiter();\n            self.public_decode_state = super::decode::DecodeState::empty();\n            self.public_op_busy_fault();"),
        "public operations must start only from Idle, checked completion must poison on wrong active-op order, and terminal sweep must use an explicit lenient clear path"
    );
    assert!(
        route_preview.contains("if self.public_active_op != PublicActiveOp::Idle")
            && route_preview.contains("self.public_op_busy_fault();")
            && route_preview.contains("return Err(SendError::PhaseInvariant);"),
        "flow preview must not overwrite a forgotten active operation"
    );
    assert!(
        endpoint_ops.contains("match unsafe { self.init_public_send_state(&init) }")
            && endpoint_ops.contains("kernel::PublicOpLease::Held => {}")
            && endpoint_ops.contains("kernel::PublicOpLease::Rejected =>")
            && endpoint_ops.contains("crate::endpoint::SendError::PhaseInvariant"),
        "flow must not return an affine Flow handle when send active-lease initialization fails after preview"
    );
    assert!(
        flow.contains("impl<'e, 'r, const ROLE: u8, M> Drop for Flow")
            && flow.contains("impl<'a, 'e, 'r, const ROLE: u8> Drop for RawSendFuture")
            && futures.contains("enum OfferFutureLease")
            && futures.contains("enum RecvFutureLease")
            && runtime_types.contains("pub(crate) enum RecvPayloadMode")
            && branch.contains("OfferFutureLease::from_public_lease")
            && futures.contains("RecvFutureLease::from_public_lease")
            && runtime_types.contains("const fn from_allows_zero_length")
            && futures.contains("RecvPayloadMode::from_allows_zero_length")
            && branch.contains("let lease = unsafe { endpoint.init_public_offer_state() };")
            && branch.contains("lease: OfferFutureLease::from_public_lease(lease)")
            && branch.contains("OfferFutureLease::Rejected =>")
            && branch.contains("self.lease = OfferFutureLease::Completed;")
            && branch.contains("self.lease == OfferFutureLease::RestoreOnDrop")
            && futures.contains("RecvFutureLease::Rejected =>")
            && futures.contains("self.lease = RecvFutureLease::Completed;")
            && futures.contains("lease: crate::endpoint::kernel::PublicOpLease")
            && futures.contains("self.lease == RecvFutureLease::RestoreOnDrop")
            && futures.contains("progress: DecodeFutureProgress")
            && futures.contains("DecodeFutureProgress::Pending")
            && futures.contains("DecodeFutureProgress::Finished")
            && futures.contains(
                "(crate::endpoint::kernel::PublicOpLease::Held, DecodeFutureProgress::Pending)"
            )
            && futures.contains("impl<'e, 'r, const ROLE: u8> Drop for RawRecvFuture")
            && futures.contains("impl<'e, 'r, const ROLE: u8> Drop for RawDecodeFuture"),
        "Drop cleanup may remain as acquired-lease cleanup, but only futures that actually acquired the resident active lease may restore it; failed constructors must report PhaseInvariant without touching another active operation's waiter/state"
    );
    for required in [
        "forgotten_flow_leaves_endpoint_fail_closed",
        "forgotten_send_future_leaves_endpoint_fail_closed",
        "forgotten_started_send_future_leaves_flow_fail_closed",
        "forgotten_recv_future_leaves_endpoint_fail_closed",
        "forgotten_started_offer_future_leaves_endpoint_fail_closed",
        "forgotten_route_branch_leaves_endpoint_fail_closed",
        "forgotten_decode_future_leaves_endpoint_fail_closed",
        "core::mem::forget",
    ] {
        assert!(
            lifecycle_tests.contains(required),
            "mem::forget behavior must be covered by runtime regression tests: {required}"
        );
    }
}
