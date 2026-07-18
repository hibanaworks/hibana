use super::common::*;

#[test]
fn public_endpoint_operations_are_drop_independent_active_leases() {
    let public_operation = format!(
        "{}\n{}",
        read("src/endpoint/kernel/core/public_operation.rs"),
        read("src/endpoint/kernel/core/public_operation/definition.rs")
    );
    let public_types = read("src/endpoint/kernel/core/public_types.rs");
    let public_state = format!("{public_operation}\n{public_types}");
    let public_ops = read("src/endpoint/kernel/public_ops.rs");
    assert!(
        !public_ops.contains("_ =>"),
        "public operation phase matches must enumerate every phase so enum growth fails closed at compile time"
    );
    let public_poll = read("src/endpoint/kernel/public_poll.rs");
    let public_runtime = format!("{public_ops}\n{public_poll}");
    let endpoint_ops = read("src/endpoint/ops.rs");
    let send = read("src/endpoint/send.rs");
    let send_route_authority = read("src/endpoint/kernel/core/send_route_authority.rs");
    let futures = read("src/endpoint/futures.rs");
    let runtime_types = read("src/endpoint/kernel/core/runtime_types.rs");
    let branch = read("src/endpoint/branch.rs");
    let route_preview = read("src/endpoint/kernel/core/send_preview.rs");
    let branch_send_preview = read("src/endpoint/kernel/core/send_preview_authority.rs");
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
    let raw_offer_constructor = branch
        .split("impl<'e, 'r, const ROLE: u8> RawOfferFuture")
        .nth(1)
        .and_then(|tail| tail.split("pub(super) fn poll_raw").next())
        .expect("raw offer constructor must stay visible");
    let offer_lease = raw_offer_constructor
        .find("let lease = endpoint.init_public_offer_state();")
        .expect("offer state must be initialized");
    let endpoint_pointer = raw_offer_constructor
        .find("let endpoint_ptr = core::ptr::from_mut(endpoint);")
        .expect("offer future must capture the endpoint pointer");
    assert!(
        offer_lease < endpoint_pointer,
        "offer initialization must finish before creating the future's raw endpoint pointer"
    );
    let offer_poll = public_poll
        .split("fn poll_public_offer")
        .nth(1)
        .and_then(|tail| tail.split("fn poll_public_recv").next())
        .expect("public offer poll body");
    assert!(
        offer_poll
            .find("self.session_fault()")
            .expect("offer fault check")
            < offer_poll
                .find("self.public_active_op != PublicActiveOp::Offer")
                .expect("offer active-op check"),
        "terminal session fault must precede offer active-op validation"
    );
    let send_preview_fault = route_preview
        .find("self.session_fault()")
        .expect("send preview fault check");
    let send_preview_active_op = route_preview
        .find("match self.public_active_op")
        .expect("send preview active-op check");
    assert!(
        send_preview_fault < send_preview_active_op
            && route_preview[send_preview_fault..send_preview_active_op]
                .contains("return Err(SendError::SessionFault(kind));")
            && route_preview.contains("if let Err(error) = &result {")
            && route_preview.contains("self.clear_endpoint_waiter(waiters);")
            && route_preview.contains("self.poison_for_send_error(error);"),
        "every failed public send preview must converge on terminal session poisoning"
    );

    for required in [
        "pub(in crate::endpoint) enum PublicActiveOp",
        "Idle",
        "Poisoned",
        "Send",
        "Recv",
        "Offer",
        "RouteBranch",
        "RestoredRouteBranch",
        "BranchRecv",
        "BranchSend",
        "public_active_op: PublicActiveOp",
    ] {
        assert!(
            public_state.contains(required),
            "endpoint kernel active-operation lease evidence missing: {required}"
        );
    }
    for (edge, exact_runtime_uses) in [
        ("BeginOffer", 1),
        ("ResumeOffer", 1),
        ("PublishRouteBranch", 1),
        ("FinishOffer", 2),
        ("ParkOffer", 1),
        ("ParkRouteBranch", 1),
        ("BeginSend", 1),
        ("BeginBranchSend", 1),
        ("FinishSend", 2),
        ("FinishBranchSend", 1),
        ("ParkBranchSend", 1),
        ("BeginRecv", 1),
        ("BeginBranchRecv", 1),
        ("FinishRecv", 2),
        ("FinishBranchRecv", 2),
        ("ParkBranchRecv", 1),
    ] {
        assert_eq!(
            public_runtime
                .matches(&format!("PublicOpEdge::{edge}"))
                .count(),
            exact_runtime_uses,
            "the production lifecycle edge inventory changed: {edge}"
        );
    }
    assert!(
        public_operation.contains("pub(crate) enum PublicOpLease")
            && public_operation.contains("Rejected = 0")
            && public_operation.contains("Held = 1")
            && public_operation.contains("Faulted = 2")
            && public_operation.contains("enum PublicOpEdge")
            && public_operation.contains("macro_rules! define_public_operation_kernel")
            && public_operation.matches("macro_rules!").count() == 1
            && public_operation.contains("&[$(Self::$phase),+]")
            && public_operation.contains("&[$(Self::$edge),+]")
            && public_operation.matches("const ALL: &'static [Self]").count() == 2
            && public_operation.contains("const fn expected(self) -> PublicActiveOp")
            && public_operation.contains("const fn next(self) -> PublicActiveOp")
            && public_operation.contains("struct PublicOpTransition")
            && public_operation.contains("fn transition(self, edge: PublicOpEdge)")
            && public_operation.contains("if self == Self::Poisoned")
            && public_operation.contains("else if self == edge.expected()")
            && public_operation.contains("PublicOpTransition::new(PublicOpLease::Rejected, Self::Poisoned)")
            && public_operation.contains("fn clear_if_current(self, expected: Self) -> Self")
            && public_operation.contains("const fn clear_terminal(self) -> Self")
            && public_operation.contains("const fn fault(self) -> Self")
            && public_runtime.contains("self.public_active_op.transition(edge)")
            && public_runtime.contains("self.public_active_op = transition.phase();")
            && public_runtime.contains("if self.public_active_op != transition.phase()")
            && !public_runtime.contains("fn start_public_op")
            && public_runtime.contains("self.public_active_op = self.public_active_op.fault();")
            && public_runtime.contains("SessionFaultKind::ProgressInvariantViolated")
            && !public_runtime.contains("fn enter_public_op(&mut self, op: PublicActiveOp) -> bool")
            && public_runtime.contains("fn transition_public_op(\n        &mut self,\n        edge: PublicOpEdge,\n    ) -> super::core::PublicOpLease")
            && !public_runtime.contains("fn finish_public_op")
            && public_runtime.contains("fn clear_public_op_if_current(&mut self, op: PublicActiveOp)")
            && public_runtime.contains("self.public_active_op.clear_if_current(op)")
            && public_runtime.contains("self.public_active_op.clear_terminal()")
            && public_runtime.contains("fn init_public_send_state(\n        &mut self,\n        init: &SendInit,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains(
                "self.transition_public_op(PublicOpEdge::BeginSend)"
            )
            && public_runtime.contains("PublicActiveOp::RouteBranch =>")
            && public_runtime.contains(
                "self.transition_public_op(PublicOpEdge::BeginBranchSend)"
            )
            && public_runtime.contains("fn publish_public_route_branch(&mut self, label: u8) -> RecvResult<u8>")
            && public_runtime.contains("transition_public_op(PublicOpEdge::PublishRouteBranch)")
            && public_runtime.contains("fn init_public_recv_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains(
                "let lease = self.transition_public_op(PublicOpEdge::BeginRecv);"
            )
            && public_runtime.contains("fn init_public_offer_state(&mut self) -> super::core::PublicOpLease")
            && public_runtime.contains("PublicActiveOp::RestoredRouteBranch if self.public_route_branch.is_some()")
            && public_runtime.contains("transition_public_op(PublicOpEdge::ResumeOffer)")
            && !public_runtime.contains("restore_materialized_route_branch")
            && public_runtime.contains(
                "super::core::PublicOpLease::Rejected | super::core::PublicOpLease::Faulted"
            )
            && public_runtime.contains("if self.public_active_op != PublicActiveOp::Offer")
            && public_runtime.matches("self.publish_public_route_branch(label)").count() == 2
            && public_runtime.contains("fn begin_public_branch_recv_state(\n        &mut self,\n    ) -> super::core::PublicOpLease")
            && public_runtime.contains("PublicActiveOp::BranchRecv")
            && public_runtime.contains("PublicActiveOp::BranchSend")
            && public_runtime.contains("self.park_public_route_branch(PublicOpEdge::ParkBranchRecv)")
            && public_runtime.contains("self.park_public_route_branch(PublicOpEdge::ParkRouteBranch)")
            && public_runtime.contains("self.park_public_route_branch(PublicOpEdge::ParkBranchSend)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Send)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Recv)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::Offer)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::BranchRecv)")
            && public_runtime.contains("self.clear_public_op_if_current(PublicActiveOp::BranchSend)")
            && public_runtime.contains("if let Some(branch) = self.public_route_branch.as_ref() {")
            && public_runtime.contains("let label = branch.branch_meta.label;")
            && public_runtime.contains("if self.public_route_branch.is_none() {\n            self.clear_endpoint_waiter(waiters);\n            self.public_branch_recv_state = super::branch_recv::BranchRecvState::empty();\n            self.public_op_busy_fault();")
            && public_runtime.contains("self.register_endpoint_waiter(waiters);")
            && !public_runtime.contains("self.register_endpoint_waiter(cx.waker())")
            && !public_ops.contains("waker.clone()"),
        "endpoint API calls must start only from Idle, checked completion must poison on wrong active-op order, and terminal sweep must use an explicit lenient clear path"
    );
    assert!(
        route_preview.contains("PublicActiveOp::Idle")
            && route_preview.contains("self.preview_branch_send_meta(target_label, target_schema)")
            && !route_preview.contains("_ =>")
            && route_preview.contains("PublicActiveOp::RestoredRouteBranch")
            && route_preview.contains("PublicActiveOp::BranchSend =>")
            && !branch_send_preview.contains("poison_for_send_error")
            && !branch_send_preview.contains("public_op_busy_fault"),
        "send preview must preserve selected-arm authority while one outer boundary owns every terminal failure"
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
            tail.split("/// Receive the next message as `M` after descriptor evidence matches")
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
            && send.contains("endpoint.begin_public_send_state(logical_label, payload_schema)")
            && send.contains("pub(crate) fn ready_error(error: SendError) -> Self")
            && send.contains("payload: Option<kernel::RawSendPayload>")
            && send.contains("state: SendFutureState")
            && !send.contains("ready_error: Option")
            && !send.contains("logical_label: 0")
            && !send.contains("RawSendPayload::empty")
            && direct_send_constructor.contains(
                "SendFuture::pending_direct(endpoint, logical_label, payload_schema, payload)"
            )
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
            && branch.contains("OfferFutureLease::RestoreOnDrop | OfferFutureLease::Faulted")
            && branch.contains("self.lease = OfferFutureLease::Completed;")
            && branch.contains("self.lease == OfferFutureLease::RestoreOnDrop")
            && futures.contains("RecvFutureLease::Rejected =>")
            && futures.contains("RecvFutureLease::RestoreOnDrop | RecvFutureLease::Faulted")
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
        "Drop cleanup may remain as acquired-lease cleanup, but only futures that actually acquired the resident active lease may restore it; rejected constructors report PhaseInvariant while terminally faulted constructors observe the preserved SessionFault without touching waiter/state"
    );
    let route_authority = send_route_authority
        .split("pub(crate) enum SendRouteAuthority")
        .nth(1)
        .and_then(|tail| tail.split("impl SendRouteAuthority").next())
        .expect("send route authority must stay explicit");
    assert!(
        route_authority.contains("None")
            && route_authority.contains("Direct { lane: u8, audit_start: u16 }")
            && route_authority.contains("lane: u8")
            && route_authority.contains("audit_start: u16")
            && !route_authority.contains("selected_routes")
            && !route_authority.contains("SelectedRouteCommitRowsRef")
            && route_authority.contains("MaterializedBranch"),
        "send route authority must distinguish direct route preview identity from materialized route branches without storing route rows"
    );
    for forbidden in ["&", "Payload", "Codec", "RawSendPayload", "Wire"] {
        assert!(
            !route_authority.contains(forbidden),
            "send route authority must not carry references, payloads, or codec hooks: {forbidden}"
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
        "ManuallyDrop::new",
    ] {
        assert!(
            lifecycle_tests.contains(required),
            "abandoned future behavior must be covered by runtime regression tests: {required}"
        );
    }
    assert!(
        !lifecycle_tests.contains("Box::pin(controller.send"),
        "abandoned send futures must not require a leaked heap allocation"
    );
}
