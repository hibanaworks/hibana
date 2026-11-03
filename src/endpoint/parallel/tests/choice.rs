use super::*;
use core::{future::Future, task::Poll};

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_local_preview_keeps_its_event_scope_inside_a_selected_parallel_arm()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};

    type Left = g::Msg<192, ()>;
    type Local = g::Msg<193, ()>;
    type Notice = g::Msg<194, u32>;
    let global = g::route(
        g::send::<0, 1, Left>(),
        g::par(g::send::<0, 0, Local>(), g::send::<0, 1, Notice>()),
    )
    .resolve::<191>();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut sender_slab = [0; 32 * 1024];
    let mut receiver_slab = [0; 32 * 1024];
    let mut sender_storage = SessionKitStorage::uninit();
    let mut receiver_storage = SessionKitStorage::uninit();
    let sender_runtime = sender_storage
        .init()
        .rendezvous(&mut sender_slab, network.clone())
        .unwrap();
    let receiver_runtime = receiver_storage
        .init()
        .rendezvous(&mut receiver_slab, network.clone())
        .unwrap();
    sender_runtime
        .set_resolver(
            &sender_program,
            ResolverRef::<191>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
    let session = SessionId::new(1698);
    let mut sender = sender_runtime.enter(session, &sender_program).unwrap();
    let mut receiver = receiver_runtime.enter(session, &receiver_program).unwrap();
    {
        (async {
            async {
                async {
                    let _: () = (async {
                        let branch = sender.offer().await.unwrap();
                        assert_eq!(branch.label(), 193);
                        drop(branch);
                        let [mut local, mut outgoing] = sender.par().unwrap();
                        let (sent, local) = async {
                            async {
                                Ok::<_, hibana::EndpointError>((outgoing.send::<Notice>(&73)).await)
                            };
                            async { Ok::<_, hibana::EndpointError>((local.recv::<Local>()).await) };
                        }
                        .await
                        .expect("all child results are data");
                        local.unwrap();
                        sent.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 194);
                        drop(branch);
                        let [mut incoming] = receiver.par().unwrap();
                        assert_eq!(incoming.recv::<Notice>().await.unwrap(), 73);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_pending_route_send_does_not_block_an_independent_receive()
-> core::result::Result<(), hibana::EndpointError> {
    type Left = g::Msg<181, u32>;
    type Right = g::Msg<182, u32>;
    type Notice = g::Msg<183, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
        g::send::<1, 0, Notice>(),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1680);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    {
        (async {
            let [mut choosing, mut notices] = sender.par().unwrap();
            let [mut receiving, mut notifying] = receiver.par().unwrap();
            network.pause_send(0);
            let sending = choosing.send::<Right>(&7);
            let mut sending = core::pin::pin!(sending);
            core::future::poll_fn(|cx| {
                assert!(sending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            notifying.send::<Notice>(&8).await.unwrap();
            assert_eq!(notices.recv::<Notice>().await.unwrap(), 8);
            network.resume_send();
            let (sent, received) = async {
                async { Ok::<_, hibana::EndpointError>((sending.as_mut()).await) };
                async {
                    Ok::<_, hibana::EndpointError>(
                        (async {
                            let branch = receiving.offer().await.unwrap();
                            assert_eq!(branch.label(), 182);
                            branch.recv::<Right>().await.unwrap()
                        })
                        .await,
                    )
                };
            }
            .await
            .expect("all child results are data");
            sent.unwrap();
            assert_eq!(received, 7);
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_resolver_preview_allows_parallel_sends_before_the_first_send_commits()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    use core::cell::Cell;

    type A = g::Msg<184, u32>;
    type B = g::Msg<185, u32>;
    type C = g::Msg<186, u32>;
    type D = g::Msg<187, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    )
    .resolve::<190>();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let decisions = Cell::new(0);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    rv.set_resolver(
        &sender_program,
        ResolverRef::<190>::decision_state(&decisions, |calls: &Cell<u32>| {
            calls.set(calls.get() + 1);
            Ok(DecisionArm::Right)
        }),
    )
    .unwrap();
    let sid = SessionId::new(1681);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    {
        (async {
            let branch = sender.offer().await.unwrap();
            assert_eq!(branch.label(), 186);
            drop(branch);
            assert!(sender.par::<3>().is_err());
            let [mut first, mut second] = sender
                .par()
                .expect("the existing resolver preview must permit independent borrows");
            network.pause_send(0);
            let first_send = first.send::<C>(&41);
            let second_send = second.send::<D>(&42);
            let mut first_send = core::pin::pin!(first_send);
            let mut second_send = core::pin::pin!(second_send);
            core::future::poll_fn(|cx| {
                assert!(first_send.as_mut().poll(cx).is_pending());
                assert!(second_send.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            network.resume_send();
            // The second arm reaches the wire while the first arm remains
            // pending and has not been polled again since transport resume.
            second_send.as_mut().await.unwrap();
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 187);
            assert_eq!(branch.recv::<D>().await.unwrap(), 42);
            let (sent, received) = async {
                async { Ok::<_, hibana::EndpointError>((first_send.as_mut()).await) };
                async { Ok::<_, hibana::EndpointError>((receiver.recv::<C>()).await) };
            }
            .await
            .expect("all child results are data");
            sent.unwrap();
            assert_eq!(received.unwrap(), 41);
            assert_eq!(decisions.get(), 1);
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_received_preview_moves_to_its_own_parallel_borrow()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<188, u32>;
    type B = g::Msg<189, u32>;
    type C = g::Msg<190, u32>;
    type D = g::Msg<191, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1682);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    {
        (async {
            sender.send::<D>(&42).await.unwrap();
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 191);
            drop(branch);
            assert!(receiver.par::<3>().is_err());
            let [mut receive_c, mut receive_d] = receiver.par().unwrap();
            let [mut send_c, sent_d] = sender.par().unwrap();
            drop(sent_d);
            let pending_c = receive_c.recv::<C>();
            let mut pending_c = core::pin::pin!(pending_c);
            core::future::poll_fn(|cx| {
                assert!(pending_c.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            assert_eq!(receive_d.recv::<D>().await.unwrap(), 42);
            let (sent, received) = async {
                async { Ok::<_, hibana::EndpointError>((send_c.send::<C>(&41)).await) };
                async { Ok::<_, hibana::EndpointError>((pending_c.as_mut()).await) };
            }
            .await
            .expect("all child results are data");
            sent.unwrap();
            assert_eq!(received.unwrap(), 41);
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_moved_preview_payload_survives_an_independent_fault()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<192, u32>;
    type B = g::Msg<193, &'static [u8]>;
    type C = g::Msg<194, u32>;
    type WrongC = g::Msg<194, u64>;
    type D = g::Msg<195, &'static [u8]>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1683);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    {
        sender
            .send::<D>(&b"borrowed frame in second fragment".as_slice())
            .await
            .unwrap();
        let branch = receiver.offer().await.unwrap();
        assert_eq!(branch.label(), 195);
        drop(branch);
        let [mut receive_c, mut receive_d] = receiver.par().unwrap();
        let [mut send_c, sent_d] = sender.par().unwrap();
        drop(sent_d);
        let data = receive_d.recv::<D>().await.unwrap();
        send_c.send::<C>(&41).await.unwrap();
        assert!(receive_c.recv::<WrongC>().await.is_err());
        assert_eq!(data, b"borrowed frame in second fragment");
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn repeated_parallel_previews_use_the_current_resolver_choice()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    use core::cell::Cell;

    type A = g::Msg<196, u32>;
    type B = g::Msg<197, u32>;
    type C = g::Msg<198, u32>;
    type D = g::Msg<199, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    )
    .resolve::<191>()
    .roll();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let desired = Cell::new(DecisionArm::Right);
    let calls = Cell::new(0);
    let policy_input = (&desired, &calls);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    rv.set_resolver(
        &sender_program,
        ResolverRef::<191>::decision_state(
            &policy_input,
            |input: &(&Cell<DecisionArm>, &Cell<u32>)| {
                input.1.set(input.1.get() + 1);
                Ok(input.0.get())
            },
        ),
    )
    .unwrap();
    let sid = SessionId::new(1684);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    {
        (async {
            for (decision, number) in [
                (DecisionArm::Right, 1),
                (DecisionArm::Left, 2),
                (DecisionArm::Right, 3),
            ] {
                desired.set(decision);
                let label = sender.offer().await.unwrap().label();
                match label {
                    196 => {
                        assert_eq!(decision, DecisionArm::Left);
                        let [mut first, mut second] = sender.par().unwrap();
                        second.send::<B>(&number).await.unwrap();
                        let observed = receiver.offer().await.unwrap().label();
                        assert_eq!(observed, 197);
                        let [mut receive_first, mut receive_second] = receiver.par().unwrap();
                        assert_eq!(receive_second.recv::<B>().await.unwrap(), number);
                        let (sent, received) = async {
                            async {
                                Ok::<_, hibana::EndpointError>((first.send::<A>(&number)).await)
                            };
                            async {
                                Ok::<_, hibana::EndpointError>((receive_first.recv::<A>()).await)
                            };
                        }
                        .await
                        .expect("all child results are data");
                        sent.unwrap();
                        assert_eq!(received.unwrap(), number);
                    }
                    198 => {
                        assert_eq!(decision, DecisionArm::Right);
                        let [mut first, mut second] = sender.par().unwrap();
                        second.send::<D>(&number).await.unwrap();
                        let observed = receiver.offer().await.unwrap().label();
                        assert_eq!(observed, 199);
                        let [mut receive_first, mut receive_second] = receiver.par().unwrap();
                        assert_eq!(receive_second.recv::<D>().await.unwrap(), number);
                        let (sent, received) = async {
                            async {
                                Ok::<_, hibana::EndpointError>((first.send::<C>(&number)).await)
                            };
                            async {
                                Ok::<_, hibana::EndpointError>((receive_first.recv::<C>()).await)
                            };
                        }
                        .await
                        .expect("all child results are data");
                        sent.unwrap();
                        assert_eq!(received.unwrap(), number);
                    }
                    _ => panic!("unexpected resolver preview label {label}"),
                }
            }
            assert_eq!(calls.get(), 3);
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_resource_view_can_preview_a_message_from_its_current_owner()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};

    type Loan<'r> = g::Msg<201, crate::Endpoint<'r, 3>>;
    type Notice = g::Msg<202, ()>;
    type Ping = g::Msg<203, ()>;
    let global = g::route(
        g::send::<0, 1, Loan>(),
        g::par(g::send::<0, 3, Notice>(), g::send::<0, 1, Ping>()),
    )
    .resolve::<201>();
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<3>(&global);
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_runtime = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_runtime = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    owner_runtime
        .set_resolver(
            &owner_program,
            ResolverRef::<201>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
    let session = SessionId::new(1699);
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    let mut resource = owner_runtime.enter(session, &resource_program).unwrap();
    {
        (async {
            owner.send::<Notice>(&()).await.unwrap();
            let branch = resource.offer().await.unwrap();
            assert_eq!(branch.label(), 202);
            branch.recv::<Notice>().await.unwrap();
            owner.send::<Ping>(&()).await.unwrap();
            let branch = worker.offer().await.unwrap();
            assert_eq!(branch.label(), 203);
            branch.recv::<Ping>().await.unwrap();
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn repeated_parallel_offers_keep_each_lanes_received_preview()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    use core::cell::Cell;

    {
        for second_sibling_first in [false, true] {
            for receive_first in [false, true] {
                type LeftFirst = g::Msg<224, u32>;
                type LeftSecond = g::Msg<225, u32>;
                type RightFirst = g::Msg<226, u32>;
                type RightSecond = g::Msg<227, u32>;
                let global = g::route(
                    g::par(g::send::<0, 1, LeftFirst>(), g::send::<0, 1, LeftSecond>()),
                    g::par(
                        g::send::<0, 1, RightFirst>(),
                        g::send::<0, 1, RightSecond>(),
                    ),
                )
                .resolve::<224>()
                .roll();
                let sender_program = project::<0>(&global);
                let receiver_program = project::<1>(&global);
                let decision = Cell::new(DecisionArm::Right);
                let resolver_calls = Cell::new(0_u32);
                let policy_input = (&decision, &resolver_calls);
                let network = TestTransport::new();
                let mut sender_slab = [0; 65536];
                let mut receiver_slab = [0; 65536];
                let mut sender_storage = SessionKitStorage::uninit();
                let mut receiver_storage = SessionKitStorage::uninit();
                let sender_runtime = sender_storage
                    .init()
                    .rendezvous(&mut sender_slab, network.clone())
                    .unwrap();
                let receiver_runtime = receiver_storage
                    .init()
                    .rendezvous(&mut receiver_slab, network.clone())
                    .unwrap();
                sender_runtime
                    .set_resolver(
                        &sender_program,
                        ResolverRef::<224>::decision_state(
                            &policy_input,
                            |input: &(&Cell<DecisionArm>, &Cell<u32>)| {
                                input.1.set(input.1.get() + 1);
                                Ok(input.0.get())
                            },
                        ),
                    )
                    .unwrap();
                let session = SessionId::new(
                    28760 + u32::from(second_sibling_first) * 2 + u32::from(receive_first),
                );
                let mut sender = sender_runtime.enter(session, &sender_program).unwrap();
                let mut receiver = receiver_runtime.enter(session, &receiver_program).unwrap();

                crate::test_support::in_process::timeout(core::time::Duration::from_secs(1), async {
                        for (selected, value) in [
                            (DecisionArm::Right, 1_u32),
                            (DecisionArm::Left, 2_u32),
                            (DecisionArm::Right, 3_u32),
                        ] {
                            decision.set(selected);
                            let right = selected == DecisionArm::Right;
                            let arm_label = if right { 226 } else { 224 };
                            let branch = sender.offer().await.unwrap();
                            assert_eq!(branch.label(), arm_label);
                            drop(branch);
                            let [send_first, send_second] = sender.par().unwrap();
                            let [mut send_arriving, mut send_later] = if second_sibling_first {
                                [send_second, send_first]
                            } else {
                                [send_first, send_second]
                            };
                            if right {
                                if second_sibling_first {
                                    send_arriving.send::<RightSecond>(&value).await.unwrap();
                                } else {
                                    send_arriving.send::<RightFirst>(&value).await.unwrap();
                                }
                            } else if second_sibling_first {
                                send_arriving.send::<LeftSecond>(&value).await.unwrap();
                            } else {
                                send_arriving.send::<LeftFirst>(&value).await.unwrap();
                            }

                            let arriving_label = arm_label + u8::from(second_sibling_first);
                            let later_label = arm_label + u8::from(!second_sibling_first);
                            // Reoffering restores the exact frame without consuming it.
                            let branch = receiver.offer().await.unwrap();
                            assert_eq!(branch.label(), arriving_label);
                            drop(branch);
                            let branch = receiver.offer().await.unwrap();
                            assert_eq!(branch.label(), arriving_label);
                            drop(branch);

                            let [recv_first, recv_second] = receiver.par().unwrap();
                            let [mut recv_arriving, mut recv_later] = if second_sibling_first {
                                [recv_second, recv_first]
                            } else {
                                [recv_first, recv_second]
                            };
                            // The other lane cannot borrow this lane's parked frame,
                            // even though its enclosing choice is already known.
                            let observed = if receive_first {
                                match hibana_runtime::race(async { core::ops::ControlFlow::Break((recv_arriving.offer()).await) }, async { core::ops::ControlFlow::Continue((recv_later.offer()).await) }).await {core::ops::ControlFlow::Break(branch) => branch.unwrap().label(),core::ops::ControlFlow::Continue(branch) => panic!(
                                        "empty sibling offered early: {:?}",
                                        branch.map(|branch| branch.label())
                                    ),}
                            } else {
                                match hibana_runtime::race(async { core::ops::ControlFlow::Break((recv_later.offer()).await) }, async { core::ops::ControlFlow::Continue((recv_arriving.offer()).await) }).await {core::ops::ControlFlow::Break(branch) => panic!(
                                        "empty sibling offered early: {:?}",
                                        branch.map(|branch| branch.label())
                                    ),core::ops::ControlFlow::Continue(branch) => branch.unwrap().label(),}
                            };
                            assert_eq!(observed, arriving_label);
                            let branch = recv_arriving.offer().await.unwrap();
                            assert_eq!(branch.label(), arriving_label);
                            let received = if right {
                                if second_sibling_first {
                                    branch.recv::<RightSecond>().await.unwrap()
                                } else {
                                    branch.recv::<RightFirst>().await.unwrap()
                                }
                            } else if second_sibling_first {
                                branch.recv::<LeftSecond>().await.unwrap()
                            } else {
                                branch.recv::<LeftFirst>().await.unwrap()
                            };
                            assert_eq!(received, value);

                            let send = async {
                                if right {
                                    if second_sibling_first {
                                        send_later.send::<RightFirst>(&value).await.unwrap();
                                    } else {
                                        send_later.send::<RightSecond>(&value).await.unwrap();
                                    }
                                } else if second_sibling_first {
                                    send_later.send::<LeftFirst>(&value).await.unwrap();
                                } else {
                                    send_later.send::<LeftSecond>(&value).await.unwrap();
                                }
                            };
                            let receive = async {
                                let branch = recv_later.offer().await.unwrap();
                                assert_eq!(branch.label(), later_label);
                                if right {
                                    if second_sibling_first {
                                        branch.recv::<RightFirst>().await.unwrap()
                                    } else {
                                        branch.recv::<RightSecond>().await.unwrap()
                                    }
                                } else if second_sibling_first {
                                    branch.recv::<LeftFirst>().await.unwrap()
                                } else {
                                    branch.recv::<LeftSecond>().await.unwrap()
                                }
                            };
                            let received = if receive_first {
                                let (received, ()) = async {
async { Ok::<_, hibana::EndpointError>((receive).await) };
 async { let _: () = (send).await;
 Ok::<_, hibana::EndpointError>(()) };
}.await.expect("all child results are data");
                                received
                            } else {
                                let ((), received) = async {
async { let _: () = (send).await;
Ok::<_, hibana::EndpointError>(()) };
 async { Ok::<_, hibana::EndpointError>((receive).await) };
}.await.expect("all child results are data");
                                received
                            };
                            assert_eq!(received, value);
                        }
                        assert_eq!(resolver_calls.get(), 3);
                    })
                    .await
                    .unwrap_or_else(|_| {
                        panic!(
                            "parallel reentry stalled: second_sibling_first={second_sibling_first}, receive_first={receive_first}"
                        )
                    });
                assert!(network.queue_is_empty());
            }
        }
    };

    Ok(())
}
