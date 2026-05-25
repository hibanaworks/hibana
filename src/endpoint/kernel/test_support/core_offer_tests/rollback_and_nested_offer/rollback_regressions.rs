use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes()
 {
    run_offer_regression_test(
        "nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(909);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        let outer_scope = worker.cursor.node_scope_id();
                        let middle_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(outer_scope, 1)
                            .expect("outer right arm should enter middle route");
                        assert_ne!(
                            middle_scope, outer_scope,
                            "passive arm navigation must descend to a child route, not recurse on the same scope"
                        );
                        let inner_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(middle_scope, 0)
                            .expect("middle left arm should enter inner route");
                        assert_ne!(
                            inner_scope, middle_scope,
                            "nested passive arm navigation must keep descending"
                        );
                        let nested_leaf_frame = frame_label_for_cursor_label(&worker.cursor, 0x51);

                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    outer_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(1),
                            "outer scope must resolve the leaf reply through first-recv dispatch"
                        );
                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    middle_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(0),
                            "middle scope must resolve the leaf reply through first-recv dispatch"
                        );

                        for (scope, expected_arm) in
                            [(outer_scope, 1u8), (middle_scope, 0u8), (inner_scope, 0u8)]
                        {
                            let frame_label_meta = endpoint_scope_frame_label_meta(
                                worker,
                                scope,
                                ScopeLoopMeta::EMPTY,
                            );
                            with_lane_set_view(&[0], |offer_lanes| {
                                worker.ingest_scope_evidence_for_offer_lanes(
                                    scope,
                                    0,
                                    offer_lanes,
                                    false,
                                    frame_label_meta,
                                );
                            });
                            worker.ingest_binding_scope_evidence(
                                scope,
                                0,
                                nested_leaf_frame,
                                false,
                                frame_label_meta,
                            );
                            assert_eq!(
                                worker.poll_arm_from_ready_mask(scope),
                                Some(Arm::new(expected_arm).expect("binary route arm")),
                                "exact nested leaf ingress must materialize Poll for scope {scope:?}"
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit()
 {
    run_offer_regression_test(
        "dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit",
        || {
            type EntryArm0SignalMsg = Msg<{ ENTRY_ARM0_SIGNAL_LABEL }, u8>;
            type EntryArm0ReplyMsg = Msg<104, u8>;
            type DynamicParentLeftSteps =
                SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
            type DynamicParentRightBodySteps = SeqSteps<
                SendOnly<0, Role<0>, Role<1>, EntryArm0SignalMsg>,
                SendOnly<0, Role<1>, Role<0>, EntryArm0ReplyMsg>,
            >;
            type DynamicParentRightSteps = SeqSteps<HintRightHead, DynamicParentRightBodySteps>;
            type DynamicParentEntrySteps =
                BranchSteps<DynamicParentLeftSteps, DynamicParentRightSteps>;
            static DYNAMIC_DECODE_PAYLOAD: [u8; 1] = [0x5a];
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

            let program: g::Program<DynamicParentEntrySteps> = g::route(
                HINT_LEFT_ARM(),
                g::seq(
                    g::send::<
                        Role<0>,
                        Role<0>,
                        Msg<
                            ROUTE_HINT_RIGHT_LABEL,
                            GenericCapToken<RouteHintRightKind>,
                            RouteHintRightKind,
                        >,
                        0,
                    >()
                    .policy::<HINT_ROUTE_POLICY_ID>(),
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
                        let sid = SessionId::new(913);
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
                        assert!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    parent_scope,
                                    0,
                                    entry_arm0_signal_frame,
                                )
                                .is_none(),
                            "dynamic parent route must not expose static Poll dispatch"
                        );
                        let (parent_arm, target_idx) = (0..worker.cursor.local_steps_len())
                            .find_map(|idx| {
                                let recv_meta = worker.cursor.try_recv_meta_at(idx)?;
                                if recv_meta.label == ENTRY_ARM0_SIGNAL_LABEL
                                    && recv_meta.scope == parent_scope
                                {
                                    Some((recv_meta.route_arm?, idx))
                                } else {
                                    None
                                }
                            })
                            .expect("dynamic parent right arm should contain the staged recv");
                        assert_eq!(parent_arm, 1);
                        worker.set_cursor_index(target_idx);
                        let recv_meta = worker
                            .cursor
                            .try_recv_meta()
                            .expect("cursor must point at recv");
                        assert_eq!(recv_meta.label, ENTRY_ARM0_SIGNAL_LABEL);
                        let before_cursor = worker.cursor.index();
                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must start without route authority"
                        );

                        let branch = MaterializedRouteBranch {
                            label: ENTRY_ARM0_SIGNAL_LABEL,
                            binding_evidence: PackedIngressEvidence::EMPTY,
                            binding_evidence_lane: u8::MAX,
                            staged_payload: Some(StagedPayload::transport_for_test(
                                recv_meta.lane,
                                Payload::new(&DYNAMIC_DECODE_PAYLOAD),
                            )),
                            branch_meta: BranchMeta {
                                scope_id: parent_scope,
                                selected_arm: parent_arm,
                                lane_wire: recv_meta.lane,
                                eff_index: recv_meta.eff_index,
                                frame_label: recv_meta.frame_label,
                                kind: BranchKind::WireRecv,
                                route_source: RouteDecisionSource::Poll,
                                poll_route_decision_authority: false,
                            },
                        };
                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut decode =
                                pin!(CursorDecode::<EntryArm0SignalMsg>::run(worker, branch));
                            match decode.as_mut().poll(&mut cx) {
                                Poll::Ready(Err(RecvError::PhaseInvariant)) => {}
                                Poll::Ready(Ok(_)) => panic!(
                                    "decode must not commit a dynamic linger parent from child frame discriminator"
                                ),
                                Poll::Ready(Err(err)) => {
                                    panic!("decode failed with unexpected error: {err:?}")
                                }
                                Poll::Pending => panic!("staged decode unexpectedly pending"),
                            }
                        }

                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must remain unselected after failed decode commit"
                        );
                        assert_eq!(
                            worker.cursor.index(),
                            before_cursor,
                            "decode commit failure must not publish cursor progress"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "decode failure must not mint ACK authority for the dynamic parent"
                        );
                    });
                });
            });
        },
    );
}
