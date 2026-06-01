use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn admin_reply_then_snapshot_reply_right_path_survives_next_iteration()
 {
    run_offer_regression_test(
        "admin_reply_then_snapshot_reply_right_path_survives_next_iteration",
        || {
            type LoopContinueMsg = Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                (),
                crate::control::cap::resource_kinds::LoopContinueKind,
            >;
            type LoopBreakMsg = Msg<
                { TEST_LOOP_BREAK_LOGICAL },
                (),
                crate::control::cap::resource_kinds::LoopBreakKind,
            >;
            type SessionRequestWireMsg = Msg<0x10, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type CheckpointMsg = Msg<{ SNAPSHOT_CONTROL_LOGICAL }, (), SnapshotControl>;
            type StaticRouteLeftMsg = Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
            type StaticRouteRightMsg = Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
            type ReplyDecisionLeftSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteLeftMsg>, SendOnly<3, 1, 0, AdminReplyMsg>>;
            type SnapshotReplyPathSteps = SeqSteps<
                SendOnly<3, 1, 1, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, 1, 1, StaticRouteLeftMsg>,
                    SeqSteps<
                        SendOnly<3, 1, 0, SnapshotCandidatesReplyMsg>,
                        SendOnly<3, 0, 0, CheckpointMsg>,
                    >,
                >,
            >;
            type ReplyDecisionRightSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteRightMsg>, SnapshotReplyPathSteps>;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, 0, 1, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, 0, 0, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, 0, 0, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                    g::send::<1, 0, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<1, 1, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                            g::seq(
                                g::send::<1, 0, SnapshotCandidatesReplyMsg, 3>(),
                                g::send::<0, 0, CheckpointMsg, 3>(),
                            ),
                        ),
                    ),
                ),
            );
            let request_exchange: g::Program<RequestExchangeSteps> =
                g::seq(g::send::<0, 1, SessionRequestWireMsg, 3>(), reply_decision);
            let loop_program: g::Program<LoopProgramSteps> = g::route(
                g::seq(g::send::<0, 0, LoopContinueMsg, 3>(), request_exchange),
                g::send::<0, 0, LoopBreakMsg, 3>(),
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
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, &()));
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
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
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
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, &()));
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
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, &()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "snapshot outer route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_left.as_mut(),
                        "snapshot category route-left send",
                    );
                }
                {
                    let mut send_reply_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
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
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, &()));
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
                                .register_rendezvous(config, transport)
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
                                    channel: Channel::new(13),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 22,
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
