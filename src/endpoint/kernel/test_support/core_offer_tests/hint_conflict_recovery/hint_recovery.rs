use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn send_preview_commit_clears_stale_route_hints_on_selected_lane()
 {
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
                                controller, &()
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn send_preview_commits_ack_route_bookkeeping_on_flow_send()
 {
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
                            controller, &()
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
                            controller, &()
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
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right()
 {
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
                        controller, &()
                    ));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), "loop continue send");
                }
                {
                    let controller = controller_slot.borrow_mut();
                    let mut route_right_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedRouteRightMsg,
                    >::run(
                        controller, &()
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
