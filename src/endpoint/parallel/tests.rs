use crate::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

#[test]
fn parallel_arms_poll_independently_and_join_the_same_parent() {
    type Start = g::Msg<41, u32>;
    type Ack = g::Msg<42, u32>;
    type Done = g::Msg<43, u32>;
    let global = g::seq(
        g::par(g::send::<0, 1, Start>(), g::send::<1, 0, Ack>()),
        g::send::<0, 1, Done>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1635);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                {
                    let (mut a_left, mut a_right) = a.par().unwrap();
                    let (mut b_left, mut b_right) = b.par().unwrap();
                    tokio::join!(
                        async {
                            assert_eq!(a_right.recv::<Ack>().await.unwrap(), 8);
                        },
                        async {
                            assert_eq!(b_left.recv::<Start>().await.unwrap(), 7);
                        },
                        async {
                            tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                            a_left.send::<Start>(&7).await.unwrap();
                        },
                        async {
                            tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                            b_right.send::<Ack>(&8).await.unwrap();
                        },
                    );
                }
                let (sent, received) = tokio::join!(a.send::<Done>(&9), b.recv::<Done>());
                sent.unwrap();
                assert_eq!(received.unwrap(), 9);
            })
            .await
            .expect("independent arm I/O must not hold the sibling pending");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_sibling_fault_preserves_a_borrowed_payload() {
    type Data = g::Msg<51, &'static [u8]>;
    type Good = g::Msg<52, u32>;
    type Wrong = g::Msg<52, u64>;
    let global = g::par(g::send::<0, 1, Data>(), g::send::<0, 1, Good>());
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1636);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let (mut a_left, mut a_right) = a.par().unwrap();
            let (mut b_left, mut b_right) = b.par().unwrap();
            a_left
                .send::<Data>(&b"borrow survives the other lane".as_slice())
                .await
                .unwrap();
            let data = b_left.recv::<Data>().await.unwrap();
            a_right.send::<Good>(&7).await.unwrap();
            assert!(b_right.recv::<Wrong>().await.is_err());
            assert_eq!(data, b"borrow survives the other lane");
        });
}

#[test]
fn nested_parallel_borrows_preserve_their_own_scope() {
    type A = g::Msg<61, u32>;
    type B = g::Msg<62, u32>;
    type C = g::Msg<63, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1637);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let (mut a_left, mut a_right) = a.par().unwrap();
                let (mut b_left, mut b_right) = b.par().unwrap();
                let (mut a_first, mut a_second) = a_left.par().unwrap();
                let (mut b_first, mut b_second) = b_left.par().unwrap();
                let (a1, b1, a2, b2, a3, b3) = tokio::join!(
                    a_first.send::<A>(&11),
                    b_first.recv::<A>(),
                    a_second.recv::<B>(),
                    b_second.send::<B>(&12),
                    a_right.send::<C>(&13),
                    b_right.recv::<C>(),
                );
                a1.unwrap();
                assert_eq!(b1.unwrap(), 11);
                assert_eq!(a2.unwrap(), 12);
                b2.unwrap();
                a3.unwrap();
                assert_eq!(b3.unwrap(), 13);
            })
            .await
            .unwrap();
        });
}

#[test]
fn parallel_route_previews_do_not_take_the_sibling_choice() {
    type A = g::Msg<71, u32>;
    type B = g::Msg<72, u32>;
    type C = g::Msg<73, u32>;
    type D = g::Msg<74, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::route(g::send::<1, 0, C>(), g::send::<1, 0, D>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1638);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let (mut a_left, mut a_right) = a.par().unwrap();
                let (mut b_left, mut b_right) = b.par().unwrap();
                tokio::join!(
                    async {
                        let branch = a_right.offer().await.unwrap();
                        assert_eq!(branch.label(), 74);
                        assert_eq!(branch.recv::<D>().await.unwrap(), 14);
                    },
                    async {
                        let branch = b_left.offer().await.unwrap();
                        assert_eq!(branch.label(), 71);
                        assert_eq!(branch.recv::<A>().await.unwrap(), 11);
                    },
                    async {
                        a_left.send::<A>(&11).await.unwrap();
                    },
                    async {
                        b_right.send::<D>(&14).await.unwrap();
                    },
                );
            })
            .await
            .unwrap();
        });
}

#[test]
fn a_rolled_parallel_body_reenters_after_both_arms_join() {
    type A = g::Msg<81, u32>;
    type B = g::Msg<82, u32>;
    type C = g::Msg<83, u32>;
    type D = g::Msg<84, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<0, 1, C>(), g::send::<1, 0, D>()),
    )
    .roll();
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1639);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                for number in [1, 2, 3] {
                    let (mut a_left, mut a_right) = a.par().unwrap();
                    let (mut b_left, mut b_right) = b.par().unwrap();
                    tokio::join!(
                        async {
                            a_left.send::<A>(&number).await.unwrap();
                            assert_eq!(a_left.recv::<B>().await.unwrap(), number + 1);
                        },
                        async {
                            a_right.send::<C>(&number).await.unwrap();
                            assert_eq!(a_right.recv::<D>().await.unwrap(), number + 2);
                        },
                        async {
                            let value = b_left.recv::<A>().await.unwrap();
                            b_left.send::<B>(&(value + 1)).await.unwrap();
                        },
                        async {
                            let value = b_right.recv::<C>().await.unwrap();
                            b_right.send::<D>(&(value + 2)).await.unwrap();
                        },
                    );
                }
            })
            .await
            .unwrap();
        });
}

#[test]
fn a_fault_wakes_a_pending_sibling_on_the_same_endpoint() {
    type Left = g::Msg<81, u32>;
    type Good = g::Msg<82, u32>;
    type Wrong = g::Msg<82, u64>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Good>());
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1643);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let (_a_left, mut a_right) = a.par().unwrap();
            let (mut b_left, mut b_right) = b.par().unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let (left, right) = tokio::join!(b_left.recv::<Left>(), async {
                    a_right.send::<Good>(&7).await.unwrap();
                    b_right.recv::<Wrong>().await
                },);
                assert!(left.is_err());
                assert!(right.is_err());
            })
            .await
            .expect("the failed arm must wake its pending sibling");
        });
}

#[test]
fn forgotten_arm_receive_is_retired_with_the_parent() {
    type Left = g::Msg<83, u32>;
    type Right = g::Msg<84, u32>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Right>());
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut b = rv.enter(SessionId::new(1644), &b_program).unwrap();
    {
        let (left, mut right) = b.par().unwrap();
        let mut pending = right.recv::<Right>();
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        assert!(
            core::future::Future::poll(core::pin::Pin::new(&mut pending), &mut cx).is_pending()
        );
        // Safe Rust can abandon a future and its arm without running Drop.
        core::mem::forget(pending);
        core::mem::forget(left);
        core::mem::forget(right);
    }
    drop(b);
}

#[test]
fn one_arm_repeats_while_the_other_arm_is_still_waiting() {
    type Request = g::Msg<111, u32>;
    type Reply = g::Msg<112, u32>;
    type Slow = g::Msg<113, u32>;
    type Done = g::Msg<114, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()).roll(),
        g::seq(g::send::<0, 1, Slow>(), g::send::<1, 0, Done>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1647);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let start = tokio::time::Instant::now();
            let (mut a_left, mut a_right) = a.par().unwrap();
            let (mut b_left, mut b_right) = b.par().unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        for value in [1, 2, 3] {
                            a_left.send::<Request>(&value).await.unwrap();
                            assert_eq!(a_left.recv::<Reply>().await.unwrap(), value + 1);
                        }
                        assert!(
                            start.elapsed() < core::time::Duration::from_secs(1),
                            "the rolled arm must finish before its sibling's delayed send"
                        );
                    },
                    async {
                        for _ in 0..3 {
                            let value = b_left.recv::<Request>().await.unwrap();
                            b_left.send::<Reply>(&(value + 1)).await.unwrap();
                        }
                    },
                    async {
                        tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                        a_right.send::<Slow>(&7).await.unwrap();
                        assert_eq!(a_right.recv::<Done>().await.unwrap(), 8);
                    },
                    async {
                        let value = b_right.recv::<Slow>().await.unwrap();
                        b_right.send::<Done>(&(value + 1)).await.unwrap();
                    },
                );
            })
            .await
            .expect("one parallel arm's roll cannot depend on its unfinished sibling");
        });
}
