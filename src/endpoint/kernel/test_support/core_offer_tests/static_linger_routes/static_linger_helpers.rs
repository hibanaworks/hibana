use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn route_ack_does_not_imply_ready_arm_evidence()
 {
    run_offer_regression_test("route_ack_does_not_imply_ready_arm_evidence", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(994);
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_slot.ptr(),
                            rv_id,
                            sid,
                            &HINT_WORKER_PROGRAM(),
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let worker = worker_slot.borrow_mut();
                let scope = worker.cursor.node_scope_id();
                assert!(!scope.is_none(), "worker must start at route scope");
                let arm = if worker.arm_has_recv(scope, 0) { 0 } else { 1 };
                worker.record_scope_ack(
                    scope,
                    RouteDecisionToken::from_ack(Arm::new(arm).expect("arm")),
                );
                assert!(
                    worker.peek_scope_ack(scope).is_some(),
                    "ack authority should be preserved"
                );
                assert!(
                    !worker.scope_has_ready_arm(scope, arm),
                    "ack authority must not become recv-ready evidence"
                );
            });
        });
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn route_ack_conflict_is_fatal_and_not_cleared_by_recovery()
 {
    run_offer_regression_test(
        "route_ack_conflict_is_fatal_and_not_cleared_by_recovery",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1010);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(0).expect("arm")),
                    );
                    worker.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(1).expect("arm")),
                    );

                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "conflicting ACK authorities must poison the scope"
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "conflicting ACK authorities must not leave a selectable authority"
                    );
                    assert!(
                        !worker.recover_scope_evidence_conflict(scope, true, false),
                        "dynamic recovery may not erase an ACK authority conflict"
                    );
                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "ACK conflict must remain observable after rejected recovery"
                    );
                });
            });
        },
    );
}
