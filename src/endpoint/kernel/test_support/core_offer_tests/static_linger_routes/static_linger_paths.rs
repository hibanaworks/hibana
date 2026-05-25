use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn static_linger_parent_route_commits_only_through_static_poll_descriptor()
 {
    run_offer_regression_test(
        "static_linger_parent_route_commits_only_through_static_poll_descriptor",
        || {
            type EntryArm0SignalMsg = Msg<{ ENTRY_ARM0_SIGNAL_LABEL }, u8>;
            type EntryArm0ReplyMsg = Msg<104, u8>;
            static STATIC_DECODE_PAYLOAD: [u8; 1] = [0x5b];
            type StaticParentEntryLeftSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
                SendOnly<0, Role<0>, Role<1>, NestedStaticOuterLeftMsg>,
            >;
            type StaticParentEntryRightBodySteps = SeqSteps<
                SendOnly<0, Role<0>, Role<1>, EntryArm0SignalMsg>,
                SendOnly<0, Role<1>, Role<0>, EntryArm0ReplyMsg>,
            >;
            type StaticParentEntryRightSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
                StaticParentEntryRightBodySteps,
            >;
            type StaticParentEntrySteps =
                BranchSteps<StaticParentEntryLeftSteps, StaticParentEntryRightSteps>;
            type ControllerEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            type WorkerEndpoint = CursorEndpoint<
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

            let program: g::Program<StaticParentEntrySteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedStaticOuterLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
                    g::seq(
                        g::send::<Role<0>, Role<1>, EntryArm0SignalMsg, 0>(),
                        g::send::<Role<1>, Role<0>, EntryArm0ReplyMsg, 0>(),
                    ),
                ),
            );
            let controller_program: RoleProgram<0> = project(&program);
            let worker_program: RoleProgram<1> = project(&program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(ControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(914);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &controller_program,
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &worker_program,
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let parent_scope = worker.cursor.node_scope_id();
                        let entry_arm0_signal_frame =
                            frame_label_for_cursor_label(&worker.cursor, ENTRY_ARM0_SIGNAL_LABEL);
                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    parent_scope,
                                    0,
                                    entry_arm0_signal_frame,
                                )
                                .map(|(arm, _)| arm),
                            Some(1),
                            "static parent must expose compiled Poll dispatch for the child frame label"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "test must start without ACK/Resolver route authority"
                        );
                        let (parent_arm, target_idx) = worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(
                                parent_scope,
                                0,
                                entry_arm0_signal_frame,
                            )
                            .expect("parent route should expose Poll dispatch");
                        assert_eq!(parent_arm, 1);
                        worker.set_cursor_index(state_index_to_usize(target_idx));
                        let recv_meta = worker
                            .cursor
                            .try_recv_meta()
                            .expect("cursor must point at child recv");
                        assert_eq!(recv_meta.label, ENTRY_ARM0_SIGNAL_LABEL);

                        let branch = MaterializedRouteBranch {
                            label: ENTRY_ARM0_SIGNAL_LABEL,
                            binding_evidence: PackedIngressEvidence::EMPTY,
                            binding_evidence_lane: u8::MAX,
                            staged_payload: Some(StagedPayload::transport_for_test(
                                recv_meta.lane,
                                Payload::new(&STATIC_DECODE_PAYLOAD),
                            )),
                            branch_meta: BranchMeta {
                                scope_id: parent_scope,
                                selected_arm: parent_arm,
                                lane_wire: recv_meta.lane,
                                eff_index: recv_meta.eff_index,
                                frame_label: recv_meta.frame_label,
                                kind: BranchKind::WireRecv,
                                route_source: RouteDecisionSource::Poll,
                                route_decision_commit_evidence:
                                    RouteDecisionCommitEvidence::CachedOrDemux,
                            },
                        };
                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut decode =
                                pin!(CursorDecode::<EntryArm0SignalMsg>::run(worker, branch));
                            let decoded = poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "static child decode commit",
                            );
                            assert_eq!(decoded, STATIC_DECODE_PAYLOAD[0]);
                        }

                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            Some(1),
                            "static parent route must commit through compiled Poll descriptor"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "static Poll commit must not synthesize ACK authority"
                        );
                        let follow_up = 0x6cu8;
                        let mut send_reply =
                            pin!(CursorSend::<EntryArm0ReplyMsg>::run(worker, &follow_up));
                        let _ = poll_ready_ok(
                            &mut cx,
                            send_reply.as_mut(),
                            "post-decode child route continuation",
                        );
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes()
 {
    run_offer_regression_test(
        "deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(910);
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                type WorkerEndpoint = CursorEndpoint<
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
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_worker_program,
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };

                let outer_scope = worker.cursor.node_scope_id();
                let middle_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(outer_scope, 1)
                    .expect("outer right arm should enter middle route");
                assert_ne!(
                    middle_scope, outer_scope,
                    "passive arm navigation must descend to a child route, not recurse on the same scope"
                );
                let third_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(middle_scope, 1)
                    .expect("middle right arm should enter third route");
                assert_ne!(
                    third_scope, middle_scope,
                    "nested passive arm navigation must keep descending"
                );
                let final_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(third_scope, 1)
                    .expect("third right arm should enter final route");
                assert_ne!(
                    final_scope, third_scope,
                    "deep passive arm navigation must keep descending"
                );
                let deep_final_frame = frame_label_for_cursor_label(&worker.cursor, 0x55);

                for scope in [outer_scope, middle_scope, third_scope] {
                    assert_eq!(
                        worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(scope, 0, deep_final_frame)
                            .map(|(arm, _)| arm),
                        Some(1),
                        "ancestor scope must resolve the deep final reply through first-recv dispatch"
                    );
                }

                let frame_label_meta =
                    endpoint_scope_frame_label_meta(worker, outer_scope, ScopeLoopMeta::EMPTY);
                with_lane_set_view(&[0], |offer_lanes| {
                    worker.ingest_scope_evidence_for_offer_lanes(
                        outer_scope,
                        0,
                        offer_lanes,
                        false,
                        frame_label_meta,
                    );
                });
                worker.ingest_binding_scope_evidence(
                    outer_scope,
                    0,
                    deep_final_frame,
                    false,
                    frame_label_meta,
                );

                for scope in [outer_scope, middle_scope, third_scope, final_scope] {
                    assert_eq!(
                        worker.poll_arm_from_ready_mask(scope),
                        Some(Arm::new(1).expect("binary route arm")),
                        "exact deep final ingress must materialize Poll for scope {scope:?}"
                    );
                    assert_eq!(
                        worker.preview_selected_arm_for_scope(scope),
                        Some(1),
                        "exact deep final ingress must seed descendant preview selection for scope {scope:?}"
                    );
                }

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn deep_right_nested_final_reply_offer_materializes_leaf_label()
 {
    run_offer_regression_test(
        "deep_right_nested_final_reply_offer_materializes_leaf_label",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(911);
                let payload = 0x55u8;
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                type WorkerEndpoint = CursorEndpoint<
                    'static,
                    1,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    TestBinding,
                >;
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_worker_program,
                            TestBinding::with_incoming_and_payloads(
                                &[IngressEvidence {
                                    frame_label: FrameLabel::new(DEEP_RIGHT_FINAL_RIGHT_FRAME),
                                    instance: 17,
                                    has_fin: false,
                                    channel: Channel::new(4),
                                }],
                                &[&[payload]],
                            ),
                        )
                        .expect("attach worker endpoint");
                }

                let waker = noop_waker_ref();
                let mut cx = Context::from_waker(waker);

                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "outer route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "middle route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "third route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "final route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<DeepRightFinalRightMsg>::run(controller, &payload)
                    );
                    let _ = poll_ready_ok(&mut cx, reply_send.as_mut(), "final right reply send");
                }

                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                let branch = {
                    let mut offer = pin!(cursor_offer(worker));
                    match offer.as_mut().poll(&mut cx) {
                        Poll::Ready(Ok(branch)) => branch,
                        Poll::Ready(Err(err)) => {
                            panic!("worker deep final offer failed: {err:?}")
                        }
                        Poll::Pending => {
                            panic!("worker deep final offer unexpectedly pending")
                        }
                    }
                };
                assert_eq!(
                    branch_label(&branch),
                    0x55,
                    "worker must materialize the deep final reply"
                );
                let mut decode = pin!(CursorDecode::<DeepRightFinalRightMsg>::run(worker, branch));
                match decode.as_mut().poll(&mut cx) {
                    Poll::Ready(Ok(reply)) => assert_eq!(reply, payload),
                    Poll::Ready(Err(err)) => {
                        panic!("worker deep final decode failed: {err:?}")
                    }
                    Poll::Pending => {
                        panic!("worker deep final decode unexpectedly pending")
                    }
                }

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress()
 {
    run_offer_regression_test(
        "deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            type DeferredCluster = SessionCluster<
                'static,
                DeferredIngressTransport,
                DefaultLabelUniverse,
                CounterClock,
                4,
            >;
            offer_fixture!(2048, clock, config);
            with_offer_value_slot!(DeferredIngressState, deferred_state_slot, {
                deferred_state_slot.store(DeferredIngressState::new());
                let deferred_state: &'static DeferredIngressState =
                    unsafe { &*deferred_state_slot.ptr() };
                with_offer_cluster!(clock, DeferredCluster, cluster_ref, {
                    let transport = DeferredIngressTransport::new(deferred_state);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(912);
                    let payload = 0x55u8;
                    type ControllerEndpoint = CursorEndpoint<
                        'static,
                        0,
                        DeferredIngressTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                        4,
                        crate::control::cap::mint::MintConfig,
                        NoBinding,
                    >;
                    type WorkerEndpoint = CursorEndpoint<
                        'static,
                        1,
                        DeferredIngressTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                        4,
                        crate::control::cap::mint::MintConfig,
                        DeferredIngressBinding,
                    >;
                    let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                                rv_id,
                                sid,
                                &deep_right_controller_program,
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                                rv_id,
                                sid,
                                &deep_right_worker_program,
                                DeferredIngressBinding::with_incoming_and_payloads(
                                    deferred_state,
                                    &[IngressEvidence {
                                        frame_label: FrameLabel::new(DEEP_RIGHT_FINAL_RIGHT_FRAME),
                                        instance: 17,
                                        has_fin: false,
                                        channel: Channel::new(4),
                                    }],
                                    &[&[payload]],
                                ),
                            )
                            .expect("attach worker endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);

                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "outer deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "middle deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "third deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "final deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut reply_send = core::pin::pin!(
                            CursorSend::<DeepRightFinalRightMsg>::run(controller, &payload)
                        );
                        let _ = poll_ready_ok(
                            &mut cx,
                            reply_send.as_mut(),
                            "final deferred right reply send",
                        );
                    }

                    let worker =
                        unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                    let branch = {
                        let mut offer = pin!(cursor_offer(worker));
                        match offer.as_mut().poll(&mut cx) {
                            Poll::Ready(Ok(branch)) => branch,
                            Poll::Ready(Err(err)) => {
                                panic!("worker deep final deferred offer failed: {err:?}")
                            }
                            Poll::Pending => {
                                panic!("worker deep final deferred offer unexpectedly pending")
                            }
                        }
                    };
                    assert_eq!(
                        branch_label(&branch),
                        0x55,
                        "worker must materialize the deep final reply after deferred binding ingress"
                    );
                    assert_eq!(
                        deferred_state.requeue_count(),
                        1,
                        "transport payload consumed before deferred binding evidence wins must be requeued exactly once"
                    );
                    let mut decode =
                        pin!(CursorDecode::<DeepRightFinalRightMsg>::run(worker, branch));
                    match decode.as_mut().poll(&mut cx) {
                        Poll::Ready(Ok(reply)) => assert_eq!(reply, payload),
                        Poll::Ready(Err(err)) => {
                            panic!("worker deep final deferred decode failed: {err:?}")
                        }
                        Poll::Pending => {
                            panic!("worker deep final deferred decode unexpectedly pending")
                        }
                    }

                    unsafe {
                        core::ptr::drop_in_place(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                        );
                        core::ptr::drop_in_place(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                        );
                    }
                });
            });
        },
    );
}
