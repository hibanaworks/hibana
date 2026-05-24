#[test]
fn admin_reply_then_snapshot_reply_right_path_survives_next_iteration() {
    run_offer_regression_test(
        "admin_reply_then_snapshot_reply_right_path_survives_next_iteration",
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
            type CheckpointMsg = Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >;
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
            type ReplyDecisionLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
            >;
            type SnapshotReplyPathSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                    SeqSteps<
                        SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                        SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                    >,
                >,
            >;
            type ReplyDecisionRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SnapshotReplyPathSteps,
            >;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                            ),
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
            fn client_send_admin_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut send_continue =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, send_continue.as_mut(), "client continue send");
                }
                {
                    let mut send_request =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &1u8));
                    let _ = poll_ready_ok(cx, send_request.as_mut(), "client admin request send");
                }
            }

            #[inline(never)]
            fn server_reply_admin_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                admin_reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut offer_request = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, offer_request.as_mut(), "server admin request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must first observe the admin request"
                );
                {
                    let mut decode_request =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let _ =
                        poll_ready_ok(cx, decode_request.as_mut(), "server admin request decode");
                }
                {
                    let mut send_route_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(cx, send_route_left.as_mut(), "admin route-left send");
                }
                {
                    let mut send_reply = core::pin::pin!(CursorSend::<AdminReplyMsg>::run(
                        server,
                        &admin_reply_payload
                    ));
                    let _ = poll_ready_ok(cx, send_reply.as_mut(), "admin reply send");
                }
            }

            #[inline(never)]
            fn client_decode_admin_reply(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let admin_branch = {
                    let mut offer_reply = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, offer_reply.as_mut(), "client admin reply offer")
                };
                assert_eq!(
                    branch_label(&admin_branch),
                    0x50,
                    "client must materialize the admin reply"
                );
                let admin_reply_scope = branch_scope(&admin_branch);
                {
                    let mut decode_reply =
                        core::pin::pin!(CursorDecode::<AdminReplyMsg>::run(client, admin_branch));
                    let _ = poll_ready_ok(cx, decode_reply.as_mut(), "client admin reply decode");
                }
                assert_eq!(
                    client.selected_arm_for_scope(admin_reply_scope),
                    None,
                    "admin reply branch scope must not survive into the next loop iteration"
                );
            }

            #[inline(never)]
            fn drive_admin_round(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                admin_reply_payload: u8,
            ) {
                client_send_admin_request(cx, client_slot);
                server_reply_admin_request(cx, server_slot, admin_reply_payload);
                client_decode_admin_reply(cx, client_slot);
            }

            #[inline(never)]
            fn client_send_snapshot_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut send_continue =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ =
                        poll_ready_ok(cx, send_continue.as_mut(), "client snapshot continue send");
                }
                {
                    let mut send_request =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &2u8));
                    let _ =
                        poll_ready_ok(cx, send_request.as_mut(), "client snapshot request send");
                }
            }

            #[inline(never)]
            fn server_reply_snapshot_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                snapshot_reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut offer_request = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, offer_request.as_mut(), "server snapshot request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must observe the snapshot request"
                );
                {
                    let mut decode_request =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let _ = poll_ready_ok(
                        cx,
                        decode_request.as_mut(),
                        "server snapshot request decode",
                    );
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "snapshot outer route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_left.as_mut(),
                        "snapshot category route-left send",
                    );
                }
                {
                    let mut send_reply_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_reply_left.as_mut(),
                        "snapshot reply route-left send",
                    );
                }
                {
                    let mut send_snapshot_reply =
                        core::pin::pin!(CursorSend::<SnapshotCandidatesReplyMsg>::run(
                            server,
                            &snapshot_reply_payload
                        ));
                    let _ = poll_ready_ok(
                        cx,
                        send_snapshot_reply.as_mut(),
                        "snapshot candidates reply send",
                    );
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let snapshot_branch = {
                    let mut offer_reply = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(
                        cx,
                        offer_reply.as_mut(),
                        "client snapshot reply offer after admin path",
                    )
                };
                assert_eq!(
                    branch_label(&snapshot_branch),
                    0x51,
                    "snapshot reply must still materialize after an earlier admin-left iteration"
                );
                {
                    let mut decode_reply = core::pin::pin!(CursorDecode::<
                        SnapshotCandidatesReplyMsg,
                    >::run(
                        client, snapshot_branch
                    ));
                    let _ = poll_ready_ok(
                        cx,
                        decode_reply.as_mut(),
                        "client snapshot reply decode after admin path",
                    );
                }
                {
                    let mut send_checkpoint =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_checkpoint.as_mut(),
                        "client snapshot checkpoint send after admin path",
                    );
                }
            }

            #[inline(never)]
            fn drive_snapshot_round(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                snapshot_reply_payload: u8,
            ) {
                client_send_snapshot_request(cx, client_slot);
                server_reply_snapshot_request(cx, server_slot, snapshot_reply_payload);
                client_decode_snapshot_reply_and_checkpoint(cx, client_slot);
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
                            let sid = SessionId::new(1010);
                            let admin_reply_payload = 0x50u8;
                            let snapshot_reply_payload = 0x51u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[admin_reply_payload], &[snapshot_reply_payload]],
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
                                let admin_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x50);
                                let snapshot_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x51);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(admin_reply_frame),
                                    instance: 21,
                                    has_fin: false,
                                    channel: Channel::new(13),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 22,
                                    has_fin: false,
                                    channel: Channel::new(14),
                                });
                            }
                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            drive_admin_round(
                                &mut cx,
                                client_slot,
                                server_slot,
                                admin_reply_payload,
                            );
                            drive_snapshot_round(
                                &mut cx,
                                client_slot,
                                server_slot,
                                snapshot_reply_payload,
                            );
                        });
                    });
                }
            );
        },
    );
}

#[test]
fn snapshot_then_commit_final_reply_survives_next_iteration() {
    run_offer_regression_test(
        "snapshot_then_commit_final_reply_survives_next_iteration",
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
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type CommitCandidatesReplyMsg = Msg<0x53, u8>;
            type CommitRejectedReplyMsg = Msg<0x54, u8>;
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
            type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
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
            type CommitRejectedBranchSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitRejectedReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitFinalBranchSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitNestedDecisionSteps =
                BranchSteps<CommitRejectedBranchSteps, CommitFinalBranchSteps>;
            type CommitReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type CommitReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                CommitNestedDecisionSteps,
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
                        g::send::<Role<1>, Role<0>, Msg<0x52, u8>, 3>(),
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
                    g::route(
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, CommitRejectedReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                            ),
                        ),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                            ),
                        ),
                    ),
                ),
            );
            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, Msg<0x50, u8>, 3>(),
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
                    poll_ready_ok(cx, server_offer.as_mut(), "server first request offer")
                };
                assert_eq!(branch_label(&branch), 0x10);
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server first request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "first outer route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_left.as_mut(),
                        "first category route-left send",
                    );
                }
                {
                    let mut send_snapshot_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_snapshot_left.as_mut(),
                        "first snapshot route-left send",
                    );
                }
                {
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<SnapshotCandidatesReplyMsg>::run(server, &reply_payload)
                    );
                    let _ = poll_ready_ok(cx, reply_send.as_mut(), "first snapshot reply send");
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client first offer")
                };
                assert_eq!(branch_label(&branch), 0x51);
                let branch_scope = branch_scope(&branch);
                {
                    let mut client_decode =
                        core::pin::pin!(CursorDecode::<SnapshotCandidatesReplyMsg>::run(
                            client, branch
                        ));
                    let reply = poll_ready_ok(cx, client_decode.as_mut(), "client first decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, checkpoint_send.as_mut(), "snapshot checkpoint send");
                }
                assert_eq!(
                    client.selected_arm_for_scope(branch_scope),
                    None,
                    "completed snapshot branch scope must not survive into the next iteration"
                );
            }

            #[inline(never)]
            fn server_reply_commit_final_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server second request offer")
                };
                assert_eq!(branch_label(&branch), 0x10);
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server second request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "second outer route-right send",
                    );
                }
                {
                    let mut send_category_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_right.as_mut(),
                        "second category route-right send",
                    );
                }
                {
                    let mut send_commit_tail_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_tail_right.as_mut(),
                        "second commit tail route-right send",
                    );
                }
                {
                    let mut send_commit_final_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_final_right.as_mut(),
                        "second commit final route-right send",
                    );
                }
                {
                    let mut reply_send = core::pin::pin!(CursorSend::<CommitFinalReplyMsg>::run(
                        server,
                        &reply_payload
                    ));
                    let _ =
                        poll_ready_ok(cx, reply_send.as_mut(), "second commit final reply send");
                }
            }

            #[inline(never)]
            fn client_decode_commit_final_reply_and_cancel(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client second offer")
                };
                assert_eq!(branch_label(&branch), 0x55);
                {
                    let mut client_decode =
                        core::pin::pin!(CursorDecode::<CommitFinalReplyMsg>::run(client, branch));
                    let reply = poll_ready_ok(cx, client_decode.as_mut(), "client second decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut cancel_send =
                        core::pin::pin!(CursorSend::<SessionCancelControlMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, cancel_send.as_mut(), "commit final cancel send");
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
                            let sid = SessionId::new(1012);
                            let snapshot_reply_payload = 0x51u8;
                            let commit_final_payload = 0x55u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[snapshot_reply_payload], &[commit_final_payload]],
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
                                let commit_final_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x55);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 41,
                                    has_fin: false,
                                    channel: Channel::new(17),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(commit_final_frame),
                                    instance: 42,
                                    has_fin: false,
                                    channel: Channel::new(18),
                                });
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                1,
                                "first continue send",
                                "first request send",
                            );
                            server_reply_snapshot_request(
                                &mut cx,
                                server_slot,
                                snapshot_reply_payload,
                            );
                            client_decode_snapshot_reply_and_checkpoint(&mut cx, client_slot);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                2,
                                "second continue send",
                                "second request send",
                            );
                            server_reply_commit_final_request(
                                &mut cx,
                                server_slot,
                                commit_final_payload,
                            );
                            client_decode_commit_final_reply_and_cancel(&mut cx, client_slot);
                        });
                    });
                }
            );
        },
    );
}

#[test]
fn dropping_pending_decode_future_preserves_preview_branch_state() {
    run_offer_regression_test(
        "dropping_pending_decode_future_preserves_preview_branch_state",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(905);
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

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                let scope = worker.cursor.node_scope_id();
                                assert!(!scope.is_none(), "worker must start at route scope");
                                scope
                            };

                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 0);
                            }

                            let worker = worker_slot.borrow_mut();
                            let before_idx = worker.cursor.index();

                            let mut cx = Context::from_waker(noop_waker_ref());
                            let branch = {
                                let mut offer = pin!(cursor_offer(worker));
                                poll_ready_ok(&mut cx, offer.as_mut(), "preview branch offer")
                            };
                            assert_eq!(
                                branch_label(&branch),
                                HINT_LEFT_DATA_LABEL,
                                "offer must preview the hinted recv branch before decode"
                            );
                            let preview_ready_mask = worker.scope_ready_arm_mask(scope);
                            let preview_ack = worker.peek_scope_ack(scope);

                            {
                                let mut decode =
                                    pin!(CursorDecode::<Msg<100, u8>>::run(worker, branch));
                                assert!(
                                    matches!(decode.as_mut().poll(&mut cx), Poll::Pending),
                                    "decode should wait on transport recv before commit"
                                );
                                drop(decode);
                            }

                            assert_eq!(
                                worker.cursor.index(),
                                before_idx,
                                "dropping a pending decode future must not advance the cursor"
                            );
                            assert_eq!(
                                worker.peek_scope_ack(scope),
                                preview_ack,
                                "dropping a pending decode future must not consume ACK authority"
                            );
                            assert_eq!(
                                worker.scope_ready_arm_mask(scope),
                                preview_ready_mask,
                                "dropping a pending decode future must not clear ready-arm evidence"
                            );
                            assert!(
                                worker.selected_arm_for_scope(scope).is_none(),
                                "dropping a pending decode future must not commit route progress"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn restoring_public_preview_branch_clears_cached_arm_slot() {
    run_offer_regression_test(
        "restoring_public_preview_branch_clears_cached_arm_slot",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(906);
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

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                let scope = worker.cursor.node_scope_id();
                                assert!(!scope.is_none(), "worker must start at route scope");
                                scope
                            };

                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 0);
                            }

                            let worker = worker_slot.borrow_mut();
                            let mut cx = Context::from_waker(noop_waker_ref());

                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => {
                                    panic!("public offer must materialize preview branch: {err:?}")
                                }
                                Poll::Pending => {
                                    panic!(
                                        "public offer must not pend once the hinted arm is ready"
                                    )
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "public offer must cache the hinted preview branch"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "public offer must park the materialized branch until decode or drop"
                            );

                            worker.restore_public_route_branch();

                            assert!(
                                worker.public_route_branch.is_none(),
                                "restoring the preview branch must clear the cached public arm slot"
                            );

                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => panic!(
                                    "re-offer after restore must rematerialize the branch: {err:?}"
                                ),
                                Poll::Pending => {
                                    panic!("re-offer after restore must not pend")
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "re-offer after restore must rematerialize the same branch from restored state"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "re-offer after restore must park a fresh preview branch"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart() {
    run_offer_regression_test(
        "static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1201);
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
                            let controller = controller_slot.borrow_mut();
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            controller.port_for_lane(0).record_route_decision(scope, 1);

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            let mut offer = pin!(cursor_offer(worker));
                            match offer.as_mut().poll(&mut cx) {
                                Poll::Ready(Ok(branch)) => {
                                    panic!(
                                        "offer must not materialize before transport ingress: {}",
                                        branch_label(&branch)
                                    )
                                }
                                Poll::Ready(Err(err)) => {
                                    panic!("offer must wait for transport ingress: {err:?}")
                                }
                                Poll::Pending => {}
                            }
                            assert_eq!(
                                transport_probe.poll_count(),
                                1,
                                "known static passive arm must park on transport once instead of frontier-restarting"
                            );
                        });
                    });
                });
            });
        },
    );
}
