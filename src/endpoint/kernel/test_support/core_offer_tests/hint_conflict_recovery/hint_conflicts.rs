use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn route_hint_conflict_recovery_clears_only_hint_conflict()
 {
    run_offer_regression_test(
        "route_hint_conflict_recovery_clears_only_hint_conflict",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1011);
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
                    let ack = RouteDecisionToken::from_ack(Arm::new(0).expect("arm"));

                    worker.record_scope_ack(scope, ack);
                    worker.record_scope_frame_hint(scope, 0, HINT_LEFT_DATA_FRAME);
                    worker.record_scope_frame_hint(scope, 0, HINT_RIGHT_DATA_FRAME);

                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "conflicting hints must be observable before recovery"
                    );
                    assert_eq!(
                        worker.peek_scope_ack(scope),
                        Some(ack),
                        "hint conflict must not erase ACK authority"
                    );
                    assert!(
                        worker.recover_scope_evidence_conflict(scope, true, false),
                        "dynamic recovery may clear hint-only conflict"
                    );
                    assert!(
                        !worker.scope_evidence_conflicted(scope),
                        "hint-only conflict must be cleared"
                    );
                    assert_eq!(
                        worker.peek_scope_ack(scope),
                        Some(ack),
                        "hint-only recovery must preserve ACK authority"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn ready_arm_mask_is_one_shot_and_cleared_on_scope_exit()
 {
    run_offer_regression_test(
        "ready_arm_mask_is_one_shot_and_cleared_on_scope_exit",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(991);
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
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        worker.mark_scope_ready_arm(scope, 0);
                        assert!(worker.scope_has_ready_arm(scope, 0));
                        worker.consume_scope_ready_arm(scope, 0);
                        assert!(
                            !worker.scope_has_ready_arm(scope, 0),
                            "arm-ready evidence must be one-shot once consumed"
                        );

                        worker.mark_scope_ready_arm(scope, 1);
                        assert_ne!(worker.scope_ready_arm_mask(scope), 0);
                        worker.clear_scope_evidence(scope);
                        assert_eq!(
                            worker.scope_ready_arm_mask(scope),
                            0,
                            "scope exit must clear arm-ready evidence"
                        );
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize()
 {
    run_offer_regression_test(
        "send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(995);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }
                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(!scope.is_none(), "controller must start at route scope");

                    let mut arm = 0u8;
                    let mut found = false;
                    while arm <= 1 {
                        if controller.arm_has_recv(scope, arm)
                            && let Some((entry, _)) =
                                controller.cursor.controller_arm_entry_by_arm(scope, arm)
                            && controller
                                .cursor
                                .try_recv_meta_at(state_index_to_usize(entry))
                                .is_none()
                        {
                            let token =
                                RouteDecisionToken::from_resolver(Arm::new(arm).expect("arm"));
                            assert!(
                                controller.route_token_has_materialization_evidence(scope, token),
                                "send/local arm entry must materialize without ready-arm evidence even when recv appears later"
                            );
                            found = true;
                            break;
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                    assert!(
                        found,
                        "expected a controller arm with send/local entry and later recv in the same arm"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn lane_offer_state_reenters_same_route_scope_using_offer_entry()
 {
    run_offer_regression_test(
        "lane_offer_state_reenters_same_route_scope_using_offer_entry",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(996);
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
                    }
                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(!scope.is_none(), "controller must start at route scope");
                    let offer_entry = controller
                        .cursor
                        .route_scope_offer_entry(scope)
                        .expect("offer entry");
                    assert!(!offer_entry.is_max(), "test requires concrete offer entry");
                    let next_idx = state_index_to_usize(offer_entry) + 1;
                    controller.set_cursor_index(next_idx);
                    let region = controller
                        .cursor
                        .scope_region_by_id(scope)
                        .expect("route scope region");
                    assert!(
                        next_idx >= region.start && next_idx < region.end,
                        "test cursor must remain inside the same route scope"
                    );

                    controller.refresh_lane_offer_state(0);
                    assert!(
                        controller.route_state.active_offer_lanes().contains(0),
                        "lane must remain pending while re-entering the same route scope"
                    );
                    assert_eq!(
                        controller.route_state.lane_offer_state(0).entry,
                        offer_entry,
                        "lane offer state must normalize to canonical route offer_entry"
                    );
                    assert_eq!(
                        controller.offer_entry_representative_lane_idx(
                            state_index_to_usize(offer_entry),
                            controller
                                .offer_entry_state_snapshot(state_index_to_usize(offer_entry))
                                .expect("offer entry state snapshot"),
                        ),
                        Some(0),
                        "offer entry index must cache a representative lane for direct lookup"
                    );
                    assert!(
                        controller.offer_entry_has_active_lanes(state_index_to_usize(offer_entry)),
                        "offer entry index must track active lanes while the route remains pending"
                    );
                    assert_eq!(
                        controller.global_active_entries().entry_at(0),
                        Some(state_index_to_usize(offer_entry)),
                        "global active-entry index must point at the canonical offer entry"
                    );
                    controller.clear_lane_offer_state(0);
                    assert!(
                        !controller.offer_entry_has_active_lanes(state_index_to_usize(offer_entry)),
                        "clearing lane offer state must detach the lane from the offer entry index"
                    );
                    assert_eq!(
                        controller.frontier_state.offer_entry_state
                            [state_index_to_usize(offer_entry)]
                        .lane_idx,
                        u8::MAX,
                        "detaching the last lane must clear the representative lane cache"
                    );
                    assert_eq!(
                        controller.global_active_entries().occupancy_mask(),
                        0,
                        "detaching the last lane must clear the global active-entry index"
                    );
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn loop_semantics_are_metadata_authority()
 {
    run_offer_regression_test("loop_semantics_are_metadata_authority", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(1005);
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_slot.ptr(),
                            rv_id,
                            sid,
                            &LOOP_SEMANTICS_CONTROLLER_PROGRAM(),
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                }

                let controller = controller_slot.borrow_mut();
                let scope = controller.cursor.node_scope_id();
                assert!(
                    !scope.is_none(),
                    "controller must start at loop route scope"
                );

                let continue_kind = controller_arm_semantic_kind(
                    &controller.cursor,
                    &controller.control_semantics(),
                    scope,
                    0,
                )
                .expect("continue arm semantic kind");
                let break_kind = controller_arm_semantic_kind(
                    &controller.cursor,
                    &controller.control_semantics(),
                    scope,
                    1,
                )
                .expect("break arm semantic kind");
                assert_eq!(continue_kind, ControlSemanticKind::LoopContinue);
                assert_eq!(break_kind, ControlSemanticKind::LoopBreak);
                assert_eq!(
                    loop_control_meaning_from_semantic(continue_kind),
                    Some(LoopControlMeaning::Continue)
                );
                assert_eq!(
                    loop_control_meaning_from_semantic(break_kind),
                    Some(LoopControlMeaning::Break)
                );
                assert_eq!(
                    controller.control_semantic_kind(ControlSemanticKind::LoopContinue),
                    ControlSemanticKind::LoopContinue
                );
                assert_eq!(
                    controller.control_semantic_kind(ControlSemanticKind::LoopBreak),
                    ControlSemanticKind::LoopBreak
                );
            });
        });
    });
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn loop_continue_then_nested_custom_route_right_send_stays_well_scoped()
 {
    run_offer_regression_test(
        "loop_continue_then_nested_custom_route_right_send_stays_well_scoped",
        || {
            #[inline(never)]
            fn send_loop_continue_then_prepare_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
            ) -> SendMeta {
                {
                    let controller = controller_slot.borrow_mut();
                    let mut continue_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedContinueMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), "loop continue send");
                }

                let controller = controller_slot.borrow_mut();
                controller
                    .preview_flow::<LoopContinueScopedRouteRightMsg>()
                    .map(|preview| preview.into_parts().0)
                    .expect("open nested route-right send after continue")
            }

            #[inline(never)]
            fn assert_route_right_meta_after_continue(
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
                route_right_meta: &SendMeta,
            ) {
                let controller = controller_slot.borrow_mut();
                let offer_lane = controller
                    .port_for_lane(route_right_meta.lane as usize)
                    .lane();
                let policy = controller
                    .control
                    .cluster()
                    .expect("cluster must remain attached")
                    .policy_mode_for(
                        RendezvousId::new(controller.rendezvous_id().raw()),
                        Lane::new(offer_lane.raw()),
                        route_right_meta.eff_index,
                        RouteHintRightKind::TAG,
                        ControlOp::RouteDecision,
                    )
                    .expect("resolve route-right policy mode");
                let controller_policy = controller
                    .cursor
                    .route_scope_controller_policy(route_right_meta.scope);

                assert!(
                    !route_right_meta.scope.is_none(),
                    "nested route-right send must stay scoped"
                );
                assert_eq!(
                    route_right_meta.route_arm,
                    Some(1),
                    "nested route-right send must preserve the selected inner arm after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
                );
                let shot = route_right_meta
                    .shot
                    .expect("nested route-right send must retain shot metadata");
                assert!(
                    controller
                        .mint_descriptor_token_bytes(
                            route_right_meta.peer,
                            shot,
                            controller
                                .port_for_lane(route_right_meta.lane as usize)
                                .lane(),
                            route_right_meta.scope,
                            0,
                            crate::global::ControlDesc::of::<RouteHintRightKind>(),
                            RouteHintRightKind::encode_handle(&(1, route_right_meta.scope.raw())),
                        )
                        .is_ok(),
                    "nested route-right canonical mint must succeed after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?} cursor_idx={} node_scope={:?}",
                    controller.cursor.index(),
                    controller.cursor.node_scope_id(),
                );
            }

            #[inline(never)]
            fn send_prepared_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
                route_right_meta: &SendMeta,
            ) {
                let controller = controller_slot.borrow_mut();
                let mut route_right_send = core::pin::pin!(CursorSend::<
                    LoopContinueScopedRouteRightMsg,
                >::run_with_meta(
                    controller, *route_right_meta, None,
                ));
                let _ = poll_ready_ok(
                    cx,
                    route_right_send.as_mut(),
                    "nested route-right send after loop continue",
                );
            }

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1006);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);
                    let route_right_meta =
                        send_loop_continue_then_prepare_route_right(&mut cx, controller_slot);
                    assert_route_right_meta_after_continue(controller_slot, &route_right_meta);
                    send_prepared_route_right(&mut cx, controller_slot, &route_right_meta);
                });
            });
        },
    );
}
