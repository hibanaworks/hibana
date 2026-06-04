use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid()
 {
    run_offer_regression_test(
        "loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid",
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
            type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
            type CommitCandidatesReplyMsg = Msg<0x53, u8>;
            type CommitFinalReplyMsg = Msg<0x55, u8>;
            type CheckpointMsg = Msg<{ SNAPSHOT_CONTROL_LOGICAL }, (), SnapshotControl>;
            type SessionCancelControlMsg = Msg<{ ABORT_CONTROL_LOGICAL }, (), AbortControl>;
            type StaticRouteLeftMsg = Msg<{ TEST_ROUTE_DECISION_LOGICAL }, (), RouteDecisionKind>;
            type StaticRouteRightMsg = Msg<ROUTE_HINT_RIGHT_LABEL, (), RouteHintRightKind>;
            type SnapshotReplyLeftSteps = SeqSteps<
                SendOnly<3, 1, 1, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, 1, 0, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, 0, 0, CheckpointMsg>,
                >,
            >;
            type SnapshotReplyRightSteps = SeqSteps<
                SendOnly<3, 1, 1, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, 1, 0, SnapshotRejectedReplyMsg>,
                    SendOnly<3, 0, 0, SessionCancelControlMsg>,
                >,
            >;
            type SnapshotReplyDecisionSteps =
                BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
            type CommitReplyLeftSteps = SeqSteps<
                SendOnly<3, 1, 1, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, 1, 0, CommitCandidatesReplyMsg>,
                    SendOnly<3, 0, 0, CheckpointMsg>,
                >,
            >;
            type CommitReplyRightSteps = SeqSteps<
                SendOnly<3, 1, 1, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, 1, 0, CommitFinalReplyMsg>,
                    SendOnly<3, 0, 0, SessionCancelControlMsg>,
                >,
            >;
            type CommitReplyDecisionSteps =
                BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
            type ReplyDecisionLeftSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteLeftMsg>, SendOnly<3, 1, 0, AdminReplyMsg>>;
            type ReplyDecisionNestedLeftSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteLeftMsg>, SnapshotReplyDecisionSteps>;
            type ReplyDecisionNestedRightSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteRightMsg>, CommitReplyDecisionSteps>;
            type ReplyDecisionNestedSteps =
                BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
            type ReplyDecisionRightSteps =
                SeqSteps<SendOnly<3, 1, 1, StaticRouteRightMsg>, ReplyDecisionNestedSteps>;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, 0, 1, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, 0, 0, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, 0, 0, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let snapshot_reply_decision: g::Program<SnapshotReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<1, 0, SnapshotCandidatesReplyMsg, 3>(),
                        g::send::<0, 0, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<1, 1, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<1, 0, SnapshotRejectedReplyMsg, 3>(),
                        g::send::<0, 0, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let commit_reply_decision: g::Program<CommitReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<1, 0, CommitCandidatesReplyMsg, 3>(),
                        g::send::<0, 0, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<1, 1, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<1, 0, CommitFinalReplyMsg, 3>(),
                        g::send::<0, 0, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                    g::send::<1, 0, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<1, 1, StaticRouteRightMsg, 3>(),
                    g::route(
                        g::seq(
                            g::send::<1, 1, StaticRouteLeftMsg, 3>(),
                            snapshot_reply_decision,
                        ),
                        g::seq(
                            g::send::<1, 1, StaticRouteRightMsg, 3>(),
                            commit_reply_decision,
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
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, &()));
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
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, &()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer reply route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
                    let _ =
                        poll_ready_ok(cx, send_category_left.as_mut(), "category route-left send");
                }
                {
                    let mut send_snapshot_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
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
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, &()));
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
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, &()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer commit route-right send",
                    );
                }
                {
                    let mut send_category_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, &()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_right.as_mut(),
                        "category commit route-right send",
                    );
                }
                {
                    let mut send_commit_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, &()));
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
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, &()));
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
                                .register_rendezvous(config, transport)
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
                                let commit_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x53);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 11,
                                    channel: Channel::new(9),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(commit_reply_frame),
                                    instance: 12,
                                    channel: Channel::new(10),
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
