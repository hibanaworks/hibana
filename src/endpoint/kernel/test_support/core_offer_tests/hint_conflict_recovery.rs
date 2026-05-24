#[test]
fn route_hint_conflict_recovery_clears_only_hint_conflict() {
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
fn ready_arm_mask_is_one_shot_and_cleared_on_scope_exit() {
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
fn send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize() {
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
fn lane_offer_state_reenters_same_route_scope_using_offer_entry() {
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
fn loop_semantics_are_metadata_authority() {
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
fn loop_continue_then_nested_custom_route_right_send_stays_well_scoped() {
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

#[test]
fn send_preview_commit_clears_stale_route_hints_on_selected_lane() {
    run_offer_regression_test(
        "send_preview_commit_clears_stale_route_hints_on_selected_lane",
        || {
            const STALE_FRAME_HINT: u8 = 77;
            let stale_frame_hint_mask = FrameLabelMask::from_frame_label(STALE_FRAME_HINT);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1008);
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
                    let route_right_meta = {
                        let controller = controller_slot.borrow_mut();
                        {
                            let mut continue_send = core::pin::pin!(CursorSend::<
                                LoopContinueScopedContinueMsg,
                            >::run(
                                controller, ()
                            ));
                            let _ = poll_ready_ok(
                                &mut cx,
                                continue_send.as_mut(),
                                "loop continue send",
                            );
                        }

                        controller
                            .preview_flow::<LoopContinueScopedRouteRightMsg>()
                            .map(|preview| preview.into_parts().0)
                            .expect("open nested route-right send after continue")
                    };

                    let controller = controller_slot.borrow_mut();
                    let lane = controller
                        .port_for_lane(route_right_meta.lane as usize)
                        .lane();
                    assert!(
                        !controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "test must start without stale route hints on the selected lane"
                    );
                    controller
                        .port_for_lane(route_right_meta.lane as usize)
                        .route_table()
                        .update_pending_frame_hint_mask_for_lane(
                            lane,
                            FrameLabelMask::EMPTY,
                            stale_frame_hint_mask,
                        );
                    assert!(
                        controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "stale route hint must be staged before the send commit"
                    );

                    {
                        let mut route_right_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedRouteRightMsg,
                        >::run_with_meta(
                            controller,
                            route_right_meta,
                            None
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right_send.as_mut(),
                            "nested route-right send clears stale route hints",
                        );
                    }

                    assert!(
                        !controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "send commit must clear offer-scoped route hints on the selected lane"
                    );
                });
            });
        },
    );
}

#[test]
fn send_preview_commits_ack_route_bookkeeping_on_flow_send() {
    run_offer_regression_test(
        "send_preview_commits_ack_route_bookkeeping_on_flow_send",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1007);
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

                    {
                        let controller = controller_slot.borrow_mut();
                        let mut continue_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedContinueMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ =
                            poll_ready_ok(&mut cx, continue_send.as_mut(), "loop continue send");
                    }

                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(
                        !scope.is_none(),
                        "controller must enter the nested route scope"
                    );

                    controller.mark_scope_ready_arm(scope, 1);
                    controller.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(1).expect("valid selected arm")),
                    );
                    assert!(
                        controller.scope_has_ready_arm_evidence(scope),
                        "test requires pending scope evidence before send-arm commit"
                    );

                    {
                        let mut route_right_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right_send.as_mut(),
                            "send right arm after preview",
                        );
                    }

                    assert!(
                        !controller.scope_has_ready_arm_evidence(scope),
                        "flow().send() must clear ready-arm scope evidence after consuming the preview"
                    );

                    let saw_ack_route_decision = OFFER_TEST_TAP.with(|tap| unsafe {
                        (&*tap.get()).iter().copied().any(|event| {
                            event.id == crate::observe::ids::ROUTE_DECISION
                                && event.arg0 == sid.raw()
                                && (event.arg1 & 0xFFFF) == 1
                                && event.causal_seq() == RouteDecisionSource::Ack.as_tap_seq()
                        })
                    });
                    assert!(
                        saw_ack_route_decision,
                        "flow().send() must emit Ack route-decision observability when it consumes a send preview"
                    );
                });
            });
        },
    );
}

#[test]
fn passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right() {
    run_offer_regression_test(
        "passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right",
        || {
            #[inline(never)]
            fn send_continue_and_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
            ) {
                {
                    let controller = controller_slot.borrow_mut();
                    let mut continue_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedContinueMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), "loop continue send");
                }
                {
                    let controller = controller_slot.borrow_mut();
                    let mut route_right_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedRouteRightMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, route_right_send.as_mut(), "nested route-right send");
                }
            }

            #[inline(never)]
            fn poll_passive_nested_offer(
                cx: &mut Context<'_>,
                worker_slot: &mut OfferValueSlotGuard<'_, OfferHintWorkerBindingEndpoint>,
            ) -> u8 {
                let worker = worker_slot.borrow_mut();
                let outer_scope = worker.cursor.node_scope_id();
                let outer_ack = worker.peek_scope_ack(outer_scope);
                let outer_ready_mask = worker.scope_ready_arm_mask(outer_scope);
                let outer_poll_ready_mask = worker.scope_poll_ready_arm_mask(outer_scope);
                let mut offer = pin!(cursor_offer(worker));
                let branch = match offer.as_mut().poll(cx) {
                    Poll::Ready(Ok(branch)) => branch,
                    Poll::Ready(Err(err)) => panic!(
                        "passive nested offer failed: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                        outer_ack, outer_ready_mask, outer_poll_ready_mask,
                    ),
                    Poll::Pending => match offer.as_mut().poll(cx) {
                        Poll::Ready(Ok(branch)) => branch,
                        Poll::Ready(Err(err)) => panic!(
                            "passive nested offer failed after retry: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                            outer_ack, outer_ready_mask, outer_poll_ready_mask,
                        ),
                        Poll::Pending => panic!("passive nested offer remained pending"),
                    },
                };
                branch_label(&branch)
            }

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1007);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &LOOP_CONTINUE_PASSIVE_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &LOOP_CONTINUE_PASSIVE_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        {
                            let worker = worker_slot.borrow_mut();
                            let right_reply_frame = frame_label_for_cursor_label(
                                &worker.cursor,
                                LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL,
                            );
                            worker.binding.incoming.push_back(IngressEvidence {
                                frame_label: FrameLabel::new(right_reply_frame),
                                instance: 1,
                                has_fin: false,
                                channel: Channel::new(7),
                            });
                        }

                        let waker = noop_waker_ref();
                        let mut cx = Context::from_waker(waker);
                        send_continue_and_route_right(&mut cx, controller_slot);
                        let label = poll_passive_nested_offer(&mut cx, worker_slot);
                        assert_eq!(
                            label, LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL,
                            "passive offer must descend into the nested right arm after continue + route-right"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid()
{
    run_offer_regression_test(
        "loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid",
        || {
            type LoopContinueMsg = Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >;
            type LoopBreakMsg = Msg<
                { TEST_LOOP_BREAK_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
                crate::control::cap::resource_kinds::LoopBreakKind,
            >;
            type SessionRequestWireMsg = Msg<0x10, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
            type CommitCandidatesReplyMsg = Msg<0x53, u8>;
            type CommitFinalReplyMsg = Msg<0x55, u8>;
            type CheckpointMsg = Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >;
            type SessionCancelControlMsg =
                Msg<{ ABORT_CONTROL_LOGICAL }, GenericCapToken<AbortControl>, AbortControl>;
            type StaticRouteLeftMsg = Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >;
            type StaticRouteRightMsg = Msg<
                ROUTE_HINT_RIGHT_LABEL,
                GenericCapToken<RouteHintRightKind>,
                RouteHintRightKind,
            >;
            type SnapshotReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type SnapshotReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotRejectedReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type SnapshotReplyDecisionSteps =
                BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
            type CommitReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type CommitReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitReplyDecisionSteps =
                BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
            type ReplyDecisionLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
            >;
            type ReplyDecisionNestedLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SnapshotReplyDecisionSteps,
            >;
            type ReplyDecisionNestedRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                CommitReplyDecisionSteps,
            >;
            type ReplyDecisionNestedSteps =
                BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
            type ReplyDecisionRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                ReplyDecisionNestedSteps,
            >;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let snapshot_reply_decision: g::Program<SnapshotReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotRejectedReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let commit_reply_decision: g::Program<CommitReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::route(
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            snapshot_reply_decision,
                        ),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                            commit_reply_decision,
                        ),
                    ),
                ),
            );
            let request_exchange: g::Program<RequestExchangeSteps> = g::seq(
                g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
                reply_decision,
            );
            let loop_program: g::Program<LoopProgramSteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                    request_exchange,
                ),
                g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
            );
            let client_program: RoleProgram<0> = project(&loop_program);
            let server_program: RoleProgram<1> = project(&loop_program);
            type ClientEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            type ServerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            #[inline(never)]
            fn client_send_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                payload: u8,
                continue_context: &str,
                request_context: &str,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut continue_send =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), continue_context);
                }
                {
                    let mut request_send =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &payload));
                    let _ = poll_ready_ok(cx, request_send.as_mut(), request_context);
                }
            }

            #[inline(never)]
            fn server_reply_snapshot_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must first observe the request"
                );
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer reply route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ =
                        poll_ready_ok(cx, send_category_left.as_mut(), "category route-left send");
                }
                {
                    let mut send_snapshot_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ =
                        poll_ready_ok(cx, send_snapshot_left.as_mut(), "snapshot route-left send");
                }
                {
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<SnapshotCandidatesReplyMsg>::run(server, &reply_payload)
                    );
                    let _ =
                        poll_ready_ok(cx, reply_send.as_mut(), "snapshot candidates reply send");
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let reply_branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client snapshot reply offer")
                };
                assert_eq!(
                    branch_label(&reply_branch),
                    0x51,
                    "client must materialize the selected snapshot candidates reply label"
                );
                let reply_branch_scope = branch_scope(&reply_branch);
                {
                    let mut client_decode = core::pin::pin!(CursorDecode::<
                        SnapshotCandidatesReplyMsg,
                    >::run(
                        client, reply_branch
                    ));
                    let reply =
                        poll_ready_ok(cx, client_decode.as_mut(), "client snapshot reply decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        checkpoint_send.as_mut(),
                        "client checkpoint control send",
                    );
                }
                assert_eq!(
                    client.selected_arm_for_scope(reply_branch_scope),
                    None,
                    "completed non-linger branch scope must not survive into next loop iteration",
                );
            }

            #[inline(never)]
            fn server_reply_commit_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server commit request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must observe the second request"
                );
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server commit request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer commit route-right send",
                    );
                }
                {
                    let mut send_category_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_right.as_mut(),
                        "category commit route-right send",
                    );
                }
                {
                    let mut send_commit_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_left.as_mut(),
                        "commit reply route-left send",
                    );
                }
                {
                    let mut commit_reply_send = core::pin::pin!(CursorSend::<
                        CommitCandidatesReplyMsg,
                    >::run(
                        server, &reply_payload
                    ));
                    let _ = poll_ready_ok(
                        cx,
                        commit_reply_send.as_mut(),
                        "commit candidates reply send",
                    );
                }
            }

            #[inline(never)]
            fn client_decode_commit_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let commit_branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client commit reply offer")
                };
                assert_eq!(
                    branch_label(&commit_branch),
                    0x53,
                    "client must materialize the selected commit candidates reply label"
                );
                {
                    let mut client_decode = core::pin::pin!(
                        CursorDecode::<CommitCandidatesReplyMsg>::run(client, commit_branch)
                    );
                    let reply =
                        poll_ready_ok(cx, client_decode.as_mut(), "client commit reply decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        checkpoint_send.as_mut(),
                        "client post-commit checkpoint send",
                    );
                }
            }

            #[inline(never)]
            fn server_offer_stays_pending(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
            ) {
                let server = server_slot.borrow_mut();
                {
                    let mut server_next_offer = core::pin::pin!(cursor_offer(server));
                    match server_next_offer.as_mut().poll(cx) {
                        Poll::Ready(Err(err)) => {
                            panic!("server next offer after commit path must not fail: {err:?}")
                        }
                        Poll::Ready(Ok(branch)) => panic!(
                            "server next offer after commit path must not spuriously materialize a branch: label={}",
                            branch_label(&branch)
                        ),
                        Poll::Pending => {}
                    }
                }
            }

            offer_fixture!(4096, clock, config);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(ClientEndpoint, client_slot, {
                        with_offer_value_slot!(ServerEndpoint, server_slot, {
                            let transport = HintOnlyTransport::new(HINT_NONE);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1008);
                            let reply_payload = 0x51u8;
                            let commit_reply_payload = 0x53u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[reply_payload], &[commit_reply_payload]],
                                        ),
                                    )
                                    .expect("attach client endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        server_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &server_program,
                                        NoBinding,
                                    )
                                    .expect("attach server endpoint");
                            }
                            {
                                let client = client_slot.borrow_mut();
                                let snapshot_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x51);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 11,
                                    has_fin: false,
                                    channel: Channel::new(9),
                                });
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                7,
                                "client continue send",
                                "client request send",
                            );
                            server_reply_snapshot_request(&mut cx, server_slot, reply_payload);
                            client_decode_snapshot_reply_and_checkpoint(&mut cx, client_slot);
                            client_send_request(
                                &mut cx,
                                client_slot,
                                8,
                                "client second continue send",
                                "client commit request send",
                            );
                            server_reply_commit_request(&mut cx, server_slot, commit_reply_payload);
                            client_decode_commit_reply_and_checkpoint(&mut cx, client_slot);
                            server_offer_stays_pending(&mut cx, server_slot);
                        });
                    });
                }
            );
        },
    );
}
