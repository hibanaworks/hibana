mod common;

use common::TestTransport;
use core::{cell::Cell, future::poll_fn, pin::pin, task::Poll};
use hibana::{
    Endpoint, EndpointError, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_selected_branch_can_preview_its_next_receive_without_advancing() {
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
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
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
    .expect("the selected branch's next receive must be observable")
    .unwrap();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_choosing_role_can_preview_its_later_reply() {
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
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(
            async {
                client.send::<Request>(&7).await?;
                assert_eq!(client.offer().await?.recv::<Reply>().await?, 8);
                Ok::<_, EndpointError>(())
            },
            async {
                let number = server.offer().await?.recv::<Request>().await?;
                server.send::<Reply>(&(number + 1)).await
            }
        )
    })
    .await
    .expect("a selected controller's incoming reply must be observable")
    .unwrap();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_committed_resolver_is_not_called_again_for_a_later_receive() {
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
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(
            async {
                client.send::<Request>(&7).await?;
                assert_eq!(calls.get(), 1);
                assert_eq!(client.offer().await?.recv::<Reply>().await?, 8);
                Ok::<_, EndpointError>(())
            },
            async {
                let number = server.offer().await?.recv::<Request>().await?;
                server.send::<Reply>(&(number + 1)).await
            }
        )
    })
    .await
    .expect("the later receive must keep the committed resolver decision")
    .unwrap();
    assert_eq!(calls.get(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_later_owned_receive_preserves_the_transferred_endpoints_continuation() {
    type Notice = g::Msg<0, ()>;
    type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
    type Other = g::Msg<2, ()>;
    type Read = g::Msg<3, u64>;
    type Value = g::Msg<4, u64>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<0, 1, Notice>(), g::send::<0, 1, Loan<'_>>()),
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
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(
            async {
                owner.send::<Notice>(&()).await?;
                owner.send::<Loan<'_>>(db).await
            },
            async {
                worker.offer().await?.recv::<Notice>().await?;
                let loan = worker.offer().await?;
                assert_eq!(loan.label(), 1);
                drop(loan);
                let mut db = worker.offer().await?.recv::<Loan<'_>>().await?;
                db.send::<Read>(&41).await?;
                assert_eq!(db.recv::<Value>().await?, 42);
                Ok::<_, EndpointError>(())
            },
            async {
                let number = service.recv::<Read>().await?;
                service.send::<Value>(&(number + 1)).await
            }
        )
    })
    .await
    .expect("the later receive must transfer one usable endpoint")
    .unwrap();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_optional_repetition_can_offer_its_required_end_after_either_reply_arm() {
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
        tokio::time::timeout(core::time::Duration::from_secs(1), async {
            tokio::try_join!(
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
                },
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
                }
            )
        })
        .await
        .expect("the required end must follow zero or more packet exchanges")
        .unwrap();
    }
}
