mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use core::{cell::Cell, future::poll_fn, pin::pin, task::Poll};
use hibana::{
    Endpoint, EndpointError,
    g::{self, Message},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_old_rolled_outcome_cannot_wait_during_the_writers_own_amendment()
-> Result<(), EndpointError> {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Outcome = g::Msg<2, u8>;
    type Rewrite = g::Msg<3, ()>;
    type Rewritten = g::Msg<4, ()>;
    type Review = g::Msg<5, ()>;
    type EditFirst = g::Msg<6, ()>;
    type EditedFirst = g::Msg<7, ()>;
    type EditSecond = g::Msg<8, ()>;
    type EditedSecond = g::Msg<9, ()>;
    type Major = g::Msg<10, ()>;
    let repair = g::seq(
        g::send::<1, 4, Major>(),
        g::seq(
            g::send::<1, 2, Outcome>(),
            g::seq(g::send::<2, 3, Rewrite>(), g::send::<3, 2, Rewritten>()),
        ),
    );
    let amend = g::seq(
        g::send::<1, 4, Review>(),
        g::seq(
            g::send::<3, 2, Outcome>(),
            g::seq(
                g::seq(g::send::<2, 3, EditFirst>(), g::send::<3, 2, EditedFirst>()),
                g::seq(
                    g::send::<2, 3, EditSecond>(),
                    g::send::<3, 2, EditedSecond>(),
                ),
            ),
        ),
    );
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::route(repair, amend).roll(), g::send::<3, 2, Outcome>()),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let p4 = project::<4>(&global);
    for repairs in 0..=2 {
        let network = TestTransport::new();
        let mut slab = vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let session = SessionId::new(1982);
        let mut owner = rv.enter(session, &p0).unwrap();
        let mut checker = rv.enter(session, &p1).unwrap();
        let mut writer = rv.enter(session, &p2).unwrap();
        let document = rv.enter(session, &p3).unwrap();
        let mut reviewer = rv.enter(session, &p4).unwrap();
        owner.send::<Loan>(document).await?;
        let mut document = checker.recv::<Loan>().await?;
        for _ in 0..repairs {
            checker.send::<Major>(&()).await?;
            reviewer.recv::<Major>().await?;
            checker.send::<Outcome>(&0).await?;
            assert_eq!(writer.recv::<Outcome>().await?, 0);
            writer.send::<Rewrite>(&()).await?;
            document.recv::<Rewrite>().await?;
            document.send::<Rewritten>(&()).await?;
            writer.recv::<Rewritten>().await?;
        }
        checker.send::<Review>(&()).await?;
        reviewer.recv::<Review>().await?;
        document.send::<Outcome>(&1).await?;
        assert_eq!(writer.recv::<Outcome>().await?, 1);
        writer.send::<EditFirst>(&()).await?;
        document.recv::<EditFirst>().await?;
        document.send::<EditedFirst>(&()).await?;
        let edited = network.newest_accepted_frame(2).header;
        writer.recv::<EditedFirst>().await?;
        {
            let mut send = pin!(document.send::<Outcome>(&2));
            let result = poll_fn(|cx| Poll::Ready(send.as_mut().poll(cx))).await;
            assert!(
                matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
                "repairs={repairs}: the final outcome needs both edits: {result:?}"
            );
        }
        assert!(network.queue_is_empty());
        {
            let mut receive = pin!(writer.recv::<Outcome>());
            let result = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
            assert!(
                matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
                "repairs={repairs}: the remaining edit needs this same endpoint: {result:?}"
            );
        }
        writer.send::<EditSecond>(&()).await?;
        document.recv::<EditSecond>().await?;
        document.send::<EditedSecond>(&()).await?;
        writer.recv::<EditedSecond>().await?;
        document.send::<Outcome>(&2).await?;
        let final_outcome = network.newest_accepted_frame(2).header;
        assert_eq!(&edited.bytes()[..7], &final_outcome.bytes()[..7]);
        assert_eq!(writer.recv::<Outcome>().await?, 2);
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_retirement_follows_a_completed_choice_in_a_different_roll_arm()
-> Result<(), EndpointError> {
    type File<'r> = g::Msg<210, Endpoint<'r, 3>>;
    type Work = g::Msg<211, ()>;
    type Ack = g::Msg<212, ()>;
    type FileNotice = g::Msg<213, ()>;
    type PeerNotice = g::Msg<214, ()>;
    type Stop = g::Msg<215, ()>;
    type ReturnFile<'r> = g::Msg<216, Endpoint<'r, 3>>;
    let global = g::seq(
        g::send::<0, 1, File>(),
        g::seq(
            g::route(
                g::seq(g::send::<1, 2, Work>(), g::send::<2, 1, Ack>()),
                g::par(g::send::<1, 3, FileNotice>(), g::send::<1, 2, PeerNotice>()),
            )
            .roll(),
            g::par(g::send::<1, 0, ReturnFile>(), g::send::<1, 2, Stop>()),
        ),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    for visits in 1..=3 {
        let network = TestTransport::new();
        let mut slab = vec![0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let session = SessionId::new(992);
        let mut issuer = rv.enter(session, &p0).unwrap();
        let file = rv.enter(session, &p3).unwrap();
        let mut worker = rv.enter(session, &p1).unwrap();
        let mut peer = rv.enter(session, &p2).unwrap();
        issuer.send::<File>(file).await?;
        let file = worker.recv::<File>().await?;
        for visit in 0..visits {
            worker.send::<Work>(&()).await?;
            peer.recv::<Work>().await?;
            peer.send::<Ack>(&()).await?;
            worker
                .recv::<Ack>()
                .await
                .unwrap_or_else(|error| panic!("visit={visit}: {error:?}"));
        }
        async {
            let [mut retire, mut stop] = worker.par()?;
            async {
                retire.send::<ReturnFile>(file).await?;
            };
            async {
                stop.send::<Stop>(&()).await?;
            };
            async {
                drop(issuer.recv::<ReturnFile>().await?);
            };
            async {
                peer.recv::<Stop>().await?;
            };
            Ok(())
        }
        .await?;
        assert!(network.queue_is_empty());
    }
    for (visits, other_arm) in (1..=3).flat_map(|visits| [false, true].map(|arm| (visits, arm))) {
        let session = SessionId::new(992);
        let network = TestTransport::new();
        let mut slab0 = vec![0; 128 * 1024];
        let mut slab1 = vec![0; 128 * 1024];
        let mut slab2 = vec![0; 128 * 1024];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let mut s2 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
        let mut issuer = rv0.enter(session, &p0).unwrap();
        let file = rv0.enter(session, &p3).unwrap();
        let mut worker = rv1.enter(session, &p1).unwrap();
        let mut peer = rv2.enter(session, &p2).unwrap();
        async {
            async {
                issuer.send::<File>(file).await.expect("initial delegation");
                let [mut retire] = issuer.par().expect("issuer's retirement fragment");
                drop(retire.recv::<ReturnFile>().await.expect("returned file"));
            };
            async {
                let mut file = worker.recv::<File>().await.expect("received file");
                for visit in 0..visits {
                    if other_arm {
                        worker.send::<FileNotice>(&()).await?;
                        async {
                            let [_sent, mut notice] = worker.par()?;
                            async {
                                notice.send::<PeerNotice>(&()).await?;
                            };
                            async {
                                file.recv::<FileNotice>().await?;
                            };
                            Ok(())
                        }
                        .await?;
                    } else {
                        hibana_runtime::yield_now().await;
                        worker.send::<Work>(&()).await.expect("work send");
                        hibana_runtime::yield_now().await;
                        worker.recv::<Ack>().await.unwrap_or_else(|error| {
                            panic!("separate runtimes visit={visit}: {error:?}")
                        });
                    }
                }
                async {
                    let [mut retire, mut stop] =
                        worker.par().expect("worker's retirement fragments");
                    async {
                        retire
                            .send::<ReturnFile>(file)
                            .await
                            .expect("return file send");
                    };
                    async {
                        stop.send::<Stop>(&()).await.expect("stop send");
                    };
                    Ok(())
                }
                .await?;
            };
            async {
                for _ in 0..visits {
                    let branch = peer.offer().await.expect("first preview");
                    assert_eq!(
                        branch.label(),
                        if other_arm {
                            PeerNotice::LOGICAL_LABEL
                        } else {
                            Work::LOGICAL_LABEL
                        }
                    );
                    drop(branch);
                    drop(peer.offer().await.expect("second preview"));
                    if other_arm {
                        let [mut notice] = peer.par()?;
                        notice.recv::<PeerNotice>().await?;
                    } else {
                        hibana_runtime::yield_now().await;
                        peer.recv::<Work>().await.expect("work receive");
                        hibana_runtime::yield_now().await;
                        peer.send::<Ack>(&()).await.expect("ack send");
                    }
                }
                let [mut stop] = peer.par().expect("peer's retirement fragment");
                stop.recv::<Stop>().await.expect("stop receive");
            };
            Ok(())
        }
        .await?;
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_fresh_roll_send_waits_for_another_views_unconsumed_reply()
-> core::result::Result<(), hibana::EndpointError> {
    type File<'r> = g::Msg<0, Endpoint<'r, 2>>;
    type Request = g::Msg<1, u64>;
    type Ack = g::Msg<2, u64>;
    type Reply = g::Msg<3, u64>;
    let global = g::seq(
        g::send::<3, 0, File>(),
        g::seq(
            g::send::<2, 1, Request>(),
            g::par(g::send::<1, 0, Ack>(), g::send::<1, 2, Reply>()),
        )
        .roll(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = TestTransport::new();
    let mut slab0 = [0; 65536];
    let mut slab1 = [0; 65536];
    let mut slab3 = [0; 65536];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s3 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv3 = s3.init().rendezvous(&mut slab3, network.clone()).unwrap();
    let sid = SessionId::new(989);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut issuer = rv3.enter(sid, &p3).unwrap();
    let file = rv3.enter(sid, &p2).unwrap();
    issuer.send::<File>(file).await?;
    let mut file = owner.recv::<File>().await?;
    let mut records = rv0.tap();
    file.send::<Request>(&0).await?;
    for visit in 0..3 {
        assert_eq!(worker.recv::<Request>().await?, visit);
        async {
            let [mut ack, mut reply] = worker.par()?;
            async {
                ack.send::<Ack>(&visit).await?;
            };
            async {
                reply.send::<Reply>(&visit).await?;
            };
            Ok(())
        }
        .await?;
        assert!(
            file.send::<Request>(&(visit + 1))
                .await
                .expect_err("the caller must consume its own current reply")
                .is_unavailable()
        );
        assert_eq!(file.recv::<Reply>().await?, visit);
        if visit < 2 {
            while records.next().is_some() {}
            let next = visit + 1;
            let mut send = pin!(file.send::<Request>(&next));
            poll_fn(|cx| {
                assert!(
                    send.as_mut().poll(cx).is_pending(),
                    "the caller's visit is complete; only its co-resident view must receive"
                );
                Poll::Ready(())
            })
            .await;
            assert!(
                records
                    .by_ref()
                    .all(|record| !matches!(record.id(), tap::ENDPOINT_SEND | tap::SCOPE_REENTRY))
            );
            assert_eq!(owner.recv::<Ack>().await?, visit);
            assert!(
                network.queue_is_empty(),
                "waiting must not send the next request"
            );
            send.await?;
        } else {
            assert_eq!(owner.recv::<Ack>().await?, visit);
        }
    }
    assert!(network.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_offer_previews_required_input_roll_reentry_and_the_final_continuation()
-> core::result::Result<(), hibana::EndpointError> {
    type Item = g::Msg<4, u64>;
    type End = g::Msg<5, u64>;
    let global = g::seq(g::send::<0, 1, Item>().roll(), g::send::<0, 1, End>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    for visits in 1..=4 {
        let network = TestTransport::new();
        let mut slab0 = [0; 16384];
        let mut slab1 = [0; 16384];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
        let sid = SessionId::new(974);
        let mut sender = rv0.enter(sid, &p0).unwrap();
        let mut receiver = rv1.enter(sid, &p1).unwrap();
        let mut records = rv1.tap();
        for visit in 0..visits {
            while records.next().is_some() {}
            {
                let mut preview = pin!(receiver.offer());
                poll_fn(|cx| {
                    assert!(preview.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
            }
            sender.send::<Item>(&visit).await?;
            let preview = receiver.offer().await?;
            assert_eq!(preview.label(), Item::LOGICAL_LABEL);
            drop(preview);
            assert!(
                records.by_ref().all(|record| !matches!(
                    record.id(),
                    tap::ENDPOINT_SEND
                        | tap::ENDPOINT_RECV
                        | tap::ROUTE_ARM_SELECTION
                        | tap::SCOPE_REENTRY
                )),
                "ordinary preview and drop must not commit or choose a branch"
            );
            let preview = receiver.offer().await?;
            let value = preview
                .recv::<Item>()
                .await
                .expect("ordinary preview commit");
            assert_eq!(value, visit);
            assert_eq!(
                records
                    .by_ref()
                    .filter(|record| record.id() == tap::ENDPOINT_RECV)
                    .count(),
                1
            );
        }
        sender.send::<End>(&77).await?;
        let preview = receiver.offer().await?;
        assert_eq!(preview.label(), End::LOGICAL_LABEL);
        drop(preview);
        assert_eq!(
            receiver
                .recv::<End>()
                .await
                .expect("typed final after dropping ordinary preview"),
            77
        );
        sender.send::<Item>(&88).await?;
        assert_eq!(receiver.offer().await?.recv::<Item>().await?, 88);
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_selected_branch_can_preview_its_next_receive_without_advancing()
-> core::result::Result<(), hibana::EndpointError> {
    type First = g::Msg<0, u64>;
    type Second = g::Msg<1, u64>;
    type Other = g::Msg<2, u64>;
    type Ack = g::Msg<3, u64>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<0, 1, First>(), g::send::<0, 1, Second>()),
            g::send::<0, 1, Other>(),
        ),
        g::send::<1, 0, Ack>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab0 = [0; 16384];
    let mut slab1 = [0; 16384];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    let sid = SessionId::new(946);
    let mut sender = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    (async {
        sender.send::<First>(&7).await?;
        assert_eq!(receiver.offer().await?.recv::<First>().await?, 7);
        {
            let mut pending = pin!(receiver.offer());
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
        sender.send::<Second>(&8).await?;
        let second = receiver.offer().await?;
        assert_eq!(second.label(), 1);
        drop(second);
        assert_eq!(receiver.offer().await?.recv::<Second>().await?, 8);
        receiver.send::<Ack>(&9).await?;
        assert_eq!(sender.recv::<Ack>().await?, 9);
        Ok::<_, EndpointError>(())
    })
    .await
    .unwrap();

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn the_choosing_role_can_preview_its_later_reply()
-> core::result::Result<(), hibana::EndpointError> {
    type Request = g::Msg<0, u64>;
    type Reply = g::Msg<1, u64>;
    type Other = g::Msg<2, u64>;
    let global = g::route(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
        g::send::<0, 1, Other>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab0 = [0; 16384];
    let mut slab1 = [0; 16384];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    let sid = SessionId::new(948);
    let mut client = rv0.enter(sid, &p0).unwrap();
    let mut server = rv1.enter(sid, &p1).unwrap();
    (async {
        async {
            async {
                client.send::<Request>(&7).await?;
                assert_eq!(client.offer().await?.recv::<Reply>().await?, 8);
                Ok::<_, EndpointError>(())
            };

            async {
                let number = server.offer().await?.recv::<Request>().await?;
                server.send::<Reply>(&(number + 1)).await
            };
        }
        .await
    })
    .await
    .unwrap();

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_committed_resolver_is_not_called_again_for_a_later_receive()
-> core::result::Result<(), hibana::EndpointError> {
    type Request = g::Msg<0, u64>;
    type Reply = g::Msg<1, u64>;
    type Other = g::Msg<2, u64>;
    let global = g::route(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
        g::send::<0, 1, Other>(),
    )
    .resolve::<1>();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let calls = Cell::new(0usize);
    let network = TestTransport::new();
    let mut slab0 = [0; 16384];
    let mut slab1 = [0; 16384];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<1>::decision_state(&calls, |calls| {
            let previous = calls.get();
            calls.set(previous + 1);
            if previous == 0 {
                Ok(DecisionArm::Left)
            } else {
                Err(ResolverError::reject())
            }
        }),
    )
    .unwrap();
    let sid = SessionId::new(950);
    let mut client = rv0.enter(sid, &p0).unwrap();
    let mut server = rv1.enter(sid, &p1).unwrap();
    (async {
        async {
            async {
                client.send::<Request>(&7).await?;
                assert_eq!(calls.get(), 1);
                assert_eq!(client.offer().await?.recv::<Reply>().await?, 8);
                Ok::<_, EndpointError>(())
            };

            async {
                let number = server.offer().await?.recv::<Request>().await?;
                server.send::<Reply>(&(number + 1)).await
            };
        }
        .await
    })
    .await
    .unwrap();
    assert_eq!(calls.get(), 1);

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_later_owned_receive_preserves_the_transferred_endpoints_continuation()
-> core::result::Result<(), hibana::EndpointError> {
    type Notice = g::Msg<0, ()>;
    type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
    type Other = g::Msg<2, ()>;
    type Read = g::Msg<3, u64>;
    type Value = g::Msg<4, u64>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<0, 1, Notice>(), g::send::<0, 1, Loan>()),
            g::send::<0, 1, Other>(),
        ),
        g::seq(g::send::<2, 3, Read>(), g::send::<3, 2, Value>()),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = TestTransport::new();
    let mut slab0 = [0; 65536];
    let mut slab1 = [0; 65536];
    let mut slab3 = [0; 65536];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s3 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv3 = s3.init().rendezvous(&mut slab3, network).unwrap();
    let sid = SessionId::new(949);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let db = rv0.enter(sid, &p2).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv3.enter(sid, &p3).unwrap();
    (async {
        async {
            async {
                owner.send::<Notice>(&()).await?;
                owner.send::<Loan>(db).await
            };

            async {
                worker.offer().await?.recv::<Notice>().await?;
                let loan = worker.offer().await?;
                assert_eq!(loan.label(), 1);
                drop(loan);
                let mut db = worker.offer().await?.recv::<Loan>().await?;
                db.send::<Read>(&41).await?;
                assert_eq!(db.recv::<Value>().await?, 42);
                Ok::<_, EndpointError>(())
            };

            async {
                let number = service.recv::<Read>().await?;
                service.send::<Value>(&(number + 1)).await
            };
        }
        .await
    })
    .await
    .unwrap();

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_optional_repetition_can_offer_its_required_end_after_either_reply_arm()
-> core::result::Result<(), hibana::EndpointError> {
    type Packet = g::Msg<0, u64>;
    type Stored = g::Msg<1, u64>;
    type Dropped = g::Msg<2, u64>;
    type End = g::Msg<3, u64>;
    type Ended = g::Msg<4, u64>;
    let global = g::seq(
        g::route(
            g::seq(
                g::seq(
                    g::send::<0, 1, Packet>(),
                    g::route(g::send::<1, 0, Stored>(), g::send::<1, 0, Dropped>()),
                )
                .roll(),
                g::send::<0, 1, End>(),
            ),
            g::send::<0, 1, End>(),
        ),
        g::send::<1, 0, Ended>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    for packet_count in [0, 2] {
        let network = TestTransport::new();
        let mut slab0 = [0; 16384];
        let mut slab1 = [0; 16384];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
        let sid = SessionId::new(947);
        let mut input = rv0.enter(sid, &p0).unwrap();
        let mut owner = rv1.enter(sid, &p1).unwrap();
        (async {
            async {
                async {
                    for packet in 0..packet_count {
                        input.send::<Packet>(&packet).await?;
                        let reply = input.offer().await?;
                        let value = if packet == 0 {
                            reply.recv::<Stored>().await?
                        } else {
                            reply.recv::<Dropped>().await?
                        };
                        assert_eq!(value, packet);
                    }
                    input.send::<End>(&7).await?;
                    assert_eq!(input.recv::<Ended>().await?, 7);
                    Ok::<_, EndpointError>(())
                };

                async {
                    loop {
                        let next = owner.offer().await?;
                        match next.label() {
                            0 => {
                                let packet = next.recv::<Packet>().await?;
                                if packet == 0 {
                                    owner.send::<Stored>(&packet).await?;
                                } else {
                                    owner.send::<Dropped>(&packet).await?;
                                }
                            }
                            3 => {
                                assert_eq!(next.recv::<End>().await?, 7);
                                break;
                            }
                            label => panic!("unexpected input {label}"),
                        }
                    }
                    owner.send::<Ended>(&7).await
                };
            }
            .await
        })
        .await
        .unwrap();
    }
    Ok(())
}

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn a_completed_visit_allows_old_incoming_but_not_new_outgoing_continuations()
-> core::result::Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type View = g::Msg<230, Endpoint<'static, 33>>;
    type Data = g::Msg<231, u32>;
    type Notice = g::Msg<232, ()>;
    type Alternative = g::Msg<233, ()>;
    type Done = g::Msg<234, ()>;
    type Private = g::Msg<235, ()>;
    type PrivateReply = g::Msg<241, ()>;
    type Acknowledged = g::Msg<242, ()>;
    type Feedback = g::Msg<236, ()>;
    type OtherFeedback = g::Msg<237, ()>;
    type DecisionLoan = g::Msg<238, Endpoint<'static, 5>>;
    type Vm = g::Msg<239, Endpoint<'static, 65>>;
    type PrivateVm = g::Msg<240, ()>;
    let global = g::seq(
        g::par(g::send::<0, 1, View>(), g::send::<0, 1, Vm>()),
        g::par(
            g::seq(
                g::par(
                    g::send::<2, 4, Done>(),
                    g::route(
                        g::seq(g::send::<5, 1, Feedback>(), g::send::<1, 33, Data>()),
                        g::seq(g::send::<5, 1, OtherFeedback>(), g::send::<1, 33, Data>()),
                    )
                    .resolve::<9033>(),
                )
                .roll(),
                g::seq(
                    g::send::<6, 2, DecisionLoan>(),
                    g::seq(
                        g::route(g::send::<2, 1, Notice>(), g::send::<2, 1, Alternative>())
                            .resolve::<9032>(),
                        g::send::<1, 2, Acknowledged>(),
                    ),
                ),
            ),
            g::par(
                g::seq(
                    g::send::<33, 7, Private>(),
                    g::send::<7, 33, PrivateReply>(),
                )
                .roll(),
                g::send::<65, 65, PrivateVm>().roll(),
            ),
        ),
    );
    let p0 = hibana::runtime::program::project::<0>(&global);
    let p1 = hibana::runtime::program::project::<1>(&global);
    let p2 = hibana::runtime::program::project::<2>(&global);
    let p3 = hibana::runtime::program::project::<33>(&global);
    let p4 = hibana::runtime::program::project::<4>(&global);
    let p5 = hibana::runtime::program::project::<5>(&global);
    let p6 = hibana::runtime::program::project::<6>(&global);
    let p65 = hibana::runtime::program::project::<65>(&global);
    let p7 = hibana::runtime::program::project::<7>(&global);
    let mut slab = [0u8; 32768];
    let selected = DecisionArm::Left;
    let trial = Cell::new(DecisionArm::Left);
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &p2,
        ResolverRef::<9032>::decision_state(&selected, |arm| Ok(*arm)),
    )
    .unwrap();
    kit.set_resolver(
        &p5,
        ResolverRef::<9033>::decision_state(&trial, |arm| Ok(arm.get())),
    )
    .unwrap();
    let session = SessionId::new(9032);
    let mut first = kit.enter(session, &p0).unwrap();
    let mut second = kit.enter(session, &p1).unwrap();
    let mut third = kit.enter(session, &p2).unwrap();
    let mut fourth = kit.enter(session, &p4).unwrap();
    let mut sixth = kit.enter(session, &p6).unwrap();
    let mut seventh = kit.enter(session, &p7).unwrap();
    let mut decision = kit.enter(session, &p5).unwrap();
    let view = kit.enter(session, &p3).unwrap();
    let vm = kit.enter(session, &p65).unwrap();
    first.send::<View>(view).await?;
    first.send::<Vm>(vm).await?;
    let (mut view, vm) = async {
        let [mut files, mut wasm] = second.par()?;
        async { files.recv::<View>().await };
        async { wasm.recv::<Vm>().await };
    }
    .await?;
    third.send::<Done>(&()).await?;
    fourth.recv::<Done>().await?;
    decision.send::<Feedback>(&()).await?;
    second.offer().await?.recv::<Feedback>().await?;
    second.send::<Data>(&5).await?;
    assert_eq!(view.recv::<Data>().await?, 5);
    view.send::<Private>(&()).await?;
    seventh.recv::<Private>().await?;
    seventh.send::<PrivateReply>(&()).await?;
    view.recv::<PrivateReply>().await?;
    trial.set(DecisionArm::Right);
    third.send::<Done>(&()).await?;
    fourth.recv::<Done>().await?;
    decision.send::<OtherFeedback>(&()).await?;
    sixth.send::<DecisionLoan>(decision).await?;
    let returned_decision = third.recv::<DecisionLoan>().await?;
    third.send::<Notice>(&()).await?;
    second.recv::<OtherFeedback>().await?;
    second.send::<Data>(&7).await?;
    view.send::<Private>(&()).await?;
    seventh.recv::<Private>().await?;
    seventh.send::<PrivateReply>(&()).await?;
    view.recv::<PrivateReply>().await?;
    let mut cx = Context::from_waker(Waker::noop());
    let branch = second.offer().await?;
    branch.recv::<Notice>().await?;
    let mut acknowledgment = core::pin::pin!(second.send::<Acknowledged>(&()));
    assert!(
        acknowledgment.as_mut().poll(&mut cx).is_pending(),
        "an old incoming continuation must not authorize a new outgoing step"
    );
    assert_eq!(view.recv::<Data>().await?, 7);
    assert!(matches!(
        acknowledgment.as_mut().poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    third.recv::<Acknowledged>().await?;
    drop(returned_decision);
    drop(vm);
    Ok(())
}

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn colocated_wire_result_must_complete_before_a_later_source_receive()
-> core::result::Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    type View = g::Msg<230, Endpoint<'static, 3>>;
    type Data = g::Msg<231, u32>;
    type Notice = g::Msg<232, ()>;
    let global = g::seq(
        g::send::<0, 1, View>(),
        g::seq(g::send::<1, 3, Data>(), g::send::<2, 1, Notice>()),
    );
    let p0 = hibana::runtime::program::project::<0>(&global);
    let p1 = hibana::runtime::program::project::<1>(&global);
    let p2 = hibana::runtime::program::project::<2>(&global);
    let p3 = hibana::runtime::program::project::<3>(&global);
    let mut slab = [0u8; 32768];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let session = SessionId::new(9031);
    let mut first = kit.enter(session, &p0).unwrap();
    let mut second = kit.enter(session, &p1).unwrap();
    let mut third = kit.enter(session, &p2).unwrap();
    let view = kit.enter(session, &p3).unwrap();
    first.send::<View>(view).await?;
    let mut view = second.recv::<View>().await?;
    second.send::<Data>(&7).await?;
    third.send::<Notice>(&()).await?;
    let mut cx = Context::from_waker(Waker::noop());
    let mut notice = core::pin::pin!(second.recv::<Notice>());
    assert!(
        notice.as_mut().poll(&mut cx).is_pending(),
        "the actor must not skip its earlier colocated receive"
    );
    assert_eq!(view.recv::<Data>().await?, 7);
    assert!(matches!(notice.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    Ok(())
}
