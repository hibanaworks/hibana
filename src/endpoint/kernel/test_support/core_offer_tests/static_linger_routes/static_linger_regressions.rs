use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn unique_ready_arm_materializes_poll_without_hint()
 {
    run_offer_regression_test("unique_ready_arm_materializes_poll_without_hint", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(908);
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

                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "no ready arm evidence must not materialize a poll arm"
                );

                worker.mark_scope_ready_arm(scope, 1);
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope).map(Arm::as_u8),
                    Some(1),
                    "a unique ready arm should materialize a poll arm"
                );

                worker.mark_scope_ready_arm(scope, 0);
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "ambiguous ready-arm evidence must not materialize a poll arm"
                );
            });
        });
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_current_arm_position_without_runtime_route_state_waits()
 {
    run_offer_regression_test(
        "passive_current_arm_position_without_runtime_route_state_waits",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(907);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let Some(PassiveArmNavigation::WithinArm { entry }) = worker
                        .cursor
                        .follow_passive_observer_arm_for_scope(scope, 0)
                    else {
                        panic!("worker should expose passive arm entry");
                    };
                    worker.set_cursor_index(state_index_to_usize(entry));
                    assert_eq!(
                        worker.selected_arm_for_scope(scope),
                        None,
                        "test requires missing runtime route state"
                    );
                    assert_eq!(
                        worker
                            .cursor
                            .typestate_node(worker.cursor.index())
                            .route_arm(),
                        Some(0),
                        "current node must carry structural arm annotation"
                    );

                    let recovered = worker.current_route_arm_authorized();
                    assert!(
                        matches!(recovered, Ok(None)),
                        "passive arm position without route evidence must wait without repairing or faulting: {recovered:?}"
                    );
                    assert_eq!(
                        worker.selected_arm_for_scope(scope),
                        None,
                        "current arm position must not restore selected arm state"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn route_decision_source_domain_is_closed()
 {
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(1),
        Some(RouteDecisionSource::Ack)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(2),
        Some(RouteDecisionSource::Resolver)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(3),
        Some(RouteDecisionSource::Poll)
    ));
    assert!(RouteDecisionSource::from_tap_seq(0).is_none());
    assert!(RouteDecisionSource::from_tap_seq(4).is_none());
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn defer_without_new_evidence_stays_pending()
 {
    let mut progress = OfferProgressState::new(crate::runtime::config::OfferProgressPolicy);
    let fingerprint = EvidenceFingerprint::new(false, false, false);
    assert_eq!(
        progress.on_defer(fingerprint),
        OfferEvidenceOutcome::NewEvidence
    );
    assert_eq!(
        progress.on_defer(fingerprint),
        OfferEvidenceOutcome::Pending
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn defer_never_promotes_to_route_authority()
 {
    let defer_tag = DeferSource::Resolver.as_audit_tag();
    assert_eq!(
        DeferSource::from_audit_tag(defer_tag),
        Some(DeferSource::Resolver)
    );
    assert!(
        RouteDecisionSource::from_tap_seq(defer_tag).is_none(),
        "defer audit tags must stay outside route authority tap tags"
    );
    for authority_tag in [
        RouteDecisionSource::Ack.as_tap_seq(),
        RouteDecisionSource::Resolver.as_tap_seq(),
        RouteDecisionSource::Poll.as_tap_seq(),
    ] {
        assert!(
            DeferSource::from_audit_tag(authority_tag).is_none(),
            "route authority tap tags must not decode as defer sources"
        );
    }
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn scope_evidence_is_one_shot_per_offer()
 {
    let token = RouteDecisionToken::from_ack(Arm::new(1).expect("arm"));
    let mut evidence = ScopeEvidence {
        ack: Some(token),
        hint_frame_label: 7,
        hint_lane: 0,
        ready_arm_mask: ScopeEvidence::ARM1_READY,
        poll_ready_arm_mask: ScopeEvidence::ARM1_READY,
        flags: 0,
    };
    let first = {
        let ack = evidence.ack;
        evidence.ack = None;
        ack
    };
    let second = evidence.ack;
    assert_eq!(first, Some(token));
    assert_eq!(second, None);
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer()
 {
    run_offer_regression_test(
        "resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(990);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
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
                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let resolver_token =
                            RouteDecisionToken::from_resolver(Arm::new(0).expect("arm"));
                        assert!(
                            !worker.route_token_has_materialization_evidence(scope, resolver_token),
                            "resolver token must not materialize without arm-ready evidence"
                        );

                        worker.mark_scope_ready_arm(scope, 0);
                        assert!(
                            worker.route_token_has_materialization_evidence(scope, resolver_token),
                            "resolver token may materialize only when selected arm has ready evidence"
                        );

                        let poll_token = RouteDecisionToken::from_poll(Arm::new(1).expect("arm"));
                        assert!(
                            !worker.route_token_has_materialization_evidence(scope, poll_token),
                            "poll token must not materialize for unready arm"
                        );

                        worker.mark_scope_ready_arm(scope, 1);
                        assert!(
                            worker.route_token_has_materialization_evidence(scope, poll_token),
                            "poll token may materialize when selected arm has ready evidence"
                        );

                        let controller_scope = controller.cursor.node_scope_id();
                        assert!(
                            !controller_scope.is_none(),
                            "controller must start at route scope"
                        );
                        let controller_recv_arm = if controller.arm_has_recv(controller_scope, 0) {
                            Some(0)
                        } else if controller.arm_has_recv(controller_scope, 1) {
                            Some(1)
                        } else {
                            None
                        };
                        if let Some(controller_arm) = controller_recv_arm {
                            let controller_resolver_token = RouteDecisionToken::from_resolver(
                                Arm::new(controller_arm).expect("arm"),
                            );
                            assert!(
                                !controller.route_token_has_materialization_evidence(
                                    controller_scope,
                                    controller_resolver_token
                                ),
                                "controller resolver token must not materialize without arm-ready evidence when recv is required"
                            );
                            controller.mark_scope_ready_arm(controller_scope, controller_arm);
                            assert!(
                                controller.route_token_has_materialization_evidence(
                                    controller_scope,
                                    controller_resolver_token
                                ),
                                "controller resolver token requires selected arm evidence as well"
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn recv_required_arm_needs_ready_arm_evidence_for_all_sources()
 {
    run_offer_regression_test(
        "recv_required_arm_needs_ready_arm_evidence_for_all_sources",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(993);
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
                    let recv_arm = if worker.arm_has_recv(scope, 0) {
                        0
                    } else if worker.arm_has_recv(scope, 1) {
                        1
                    } else {
                        return;
                    };
                    let ack_token = RouteDecisionToken::from_ack(Arm::new(recv_arm).expect("arm"));
                    let resolver_token =
                        RouteDecisionToken::from_resolver(Arm::new(recv_arm).expect("arm"));
                    let poll_token =
                        RouteDecisionToken::from_poll(Arm::new(recv_arm).expect("arm"));
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, ack_token),
                        "ack token must not materialize recv-required arm without ready-arm evidence"
                    );
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, resolver_token),
                        "resolver token must not materialize recv-required arm without ready-arm evidence"
                    );
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, poll_token),
                        "poll token must not materialize recv-required arm without ready-arm evidence"
                    );
                    worker.mark_scope_ready_arm(scope, recv_arm);
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, ack_token),
                        "ack token may materialize recv-required arm when selected arm is ready"
                    );
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, resolver_token),
                        "resolver token may materialize recv-required arm when selected arm is ready"
                    );
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, poll_token),
                        "poll token may materialize recv-required arm when selected arm is ready"
                    );
                });
            });
        },
    );
}
