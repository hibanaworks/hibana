use super::*;

#[test]
fn right_associated_parallel_borrows_have_the_same_flat_local_shape() {
    type A = g::Msg<151, u32>;
    type B = g::Msg<152, u32>;
    type C = g::Msg<153, u32>;
    let global = g::par(
        g::send::<0, 1, A>(),
        g::par(g::send::<1, 0, B>(), g::send::<0, 1, C>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1669);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let [mut a_first, mut a_second, mut a_third] = a.par().unwrap();
                let [mut b_first, mut b_second, mut b_third] = b.par().unwrap();
                let (b1, a2, b3, a1, b2, a3) = tokio::join!(
                    b_first.recv::<A>(),
                    a_second.recv::<B>(),
                    b_third.recv::<C>(),
                    a_first.send::<A>(&11),
                    b_second.send::<B>(&12),
                    a_third.send::<C>(&13),
                );
                assert_eq!(b1.unwrap(), 11);
                assert_eq!(a2.unwrap(), 12);
                assert_eq!(b3.unwrap(), 13);
                a1.unwrap();
                b2.unwrap();
                a3.unwrap();
            })
            .await
            .expect("associative global grouping must not serialize local arms");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_sequential_prefix_keeps_its_later_parallel_phase() {
    type Begin = g::Msg<154, ()>;
    type A = g::Msg<155, u32>;
    type B = g::Msg<156, u32>;
    type C = g::Msg<157, u32>;
    let global = g::par(
        g::seq(
            g::send::<0, 1, Begin>(),
            g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        ),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1670);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let [mut a_work, mut a_tail] = a.par().unwrap();
            let [mut b_work, mut b_tail] = b.par().unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        a_work.send::<Begin>(&()).await.unwrap();
                        let [mut request, mut notice] = a_work.par().unwrap();
                        let (received, sent) =
                            tokio::join!(notice.recv::<B>(), request.send::<A>(&7));
                        assert_eq!(received.unwrap(), 8);
                        sent.unwrap();
                    },
                    async {
                        b_work.recv::<Begin>().await.unwrap();
                        let [mut request, mut notice] = b_work.par().unwrap();
                        let (received, sent) =
                            tokio::join!(request.recv::<A>(), notice.send::<B>(&8));
                        assert_eq!(received.unwrap(), 7);
                        sent.unwrap();
                    },
                    async {
                        tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                        a_tail.send::<C>(&9).await.unwrap();
                    },
                    async {
                        assert_eq!(b_tail.recv::<C>().await.unwrap(), 9);
                    },
                );
            })
            .await
            .expect("the sequence must enable its own nested parallel phase");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_repeated_parallel_fragment_keeps_its_iteration_boundary() {
    type A = g::Msg<158, u32>;
    type B = g::Msg<159, u32>;
    type C = g::Msg<160, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()).roll(),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1671);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let start = tokio::time::Instant::now();
            let [mut a_cycles, mut a_tail] = a.par().unwrap();
            let [mut b_cycles, mut b_tail] = b.par().unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        for value in [1, 2, 3] {
                            let [mut request, mut notice] = a_cycles.par().unwrap();
                            let (received, sent) =
                                tokio::join!(notice.recv::<B>(), request.send::<A>(&value));
                            assert_eq!(received.unwrap(), value + 10);
                            sent.unwrap();
                        }
                        assert!(start.elapsed() < core::time::Duration::from_secs(1));
                    },
                    async {
                        for value in [1, 2, 3] {
                            let [mut request, mut notice] = b_cycles.par().unwrap();
                            let reply = value + 10;
                            let (received, sent) =
                                tokio::join!(request.recv::<A>(), notice.send::<B>(&reply));
                            assert_eq!(received.unwrap(), value);
                            sent.unwrap();
                        }
                    },
                    async {
                        tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                        a_tail.send::<C>(&9).await.unwrap();
                    },
                    async {
                        assert_eq!(b_tail.recv::<C>().await.unwrap(), 9);
                    },
                );
            })
            .await
            .expect("the repeated fragment must stay independent of its sibling");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_role_without_an_arm_does_not_require_an_extra_borrow() {
    type A = g::Msg<161, u32>;
    type Hidden = g::Msg<162, u32>;
    type B = g::Msg<163, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<2, 3, Hidden>()),
        g::send::<1, 0, B>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let c_program = project::<2>(&global);
    let d_program = project::<3>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1672);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    let mut c = rv.enter(sid, &c_program).unwrap();
    let mut d = rv.enter(sid, &d_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                assert!(a.par::<3>().is_err());
                let [mut request, mut notice] = a.par().unwrap();
                let [mut peer_request, mut peer_notice] = b.par().unwrap();
                tokio::join!(
                    async {
                        request.send::<A>(&7).await.unwrap();
                        assert_eq!(peer_request.recv::<A>().await.unwrap(), 7);
                    },
                    async {
                        peer_notice.send::<B>(&8).await.unwrap();
                        assert_eq!(notice.recv::<B>().await.unwrap(), 8);
                    },
                    async {
                        c.send::<Hidden>(&9).await.unwrap();
                        assert_eq!(d.recv::<Hidden>().await.unwrap(), 9);
                    },
                );
            })
            .await
            .expect("projection must erase the unrelated role pair from these borrows");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn borrowing_parallel_arms_does_not_choose_an_unresolved_route() {
    type A = g::Msg<164, u32>;
    type B = g::Msg<165, u32>;
    type C = g::Msg<166, u32>;
    type D = g::Msg<167, u32>;
    type E = g::Msg<168, u32>;
    type F = g::Msg<169, u32>;
    let global = g::route(
        g::par(
            g::send::<0, 1, A>(),
            g::par(g::send::<0, 1, B>(), g::send::<0, 1, C>()),
        ),
        g::par(
            g::send::<0, 1, D>(),
            g::par(g::send::<0, 1, E>(), g::send::<0, 1, F>()),
        ),
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
    let sid = SessionId::new(1674);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    assert!(
        sender.par::<3>().is_err(),
        "borrowing must not pick a route for its controller"
    );
    assert!(
        receiver.par::<3>().is_err(),
        "borrowing must not guess a route before its first frame"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                sender.send::<D>(&17).await.unwrap();
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), 167);
                assert_eq!(branch.recv::<D>().await.unwrap(), 17);
                let [sent, mut send_e, mut send_f] = sender.par().unwrap();
                let [received, mut recv_e, mut recv_f] = receiver.par().unwrap();
                drop((sent, received));
                let (e, f, se, sf) = tokio::join!(
                    recv_e.recv::<E>(),
                    recv_f.recv::<F>(),
                    send_e.send::<E>(&18),
                    send_f.send::<F>(&19),
                );
                assert_eq!(e.unwrap(), 18);
                assert_eq!(f.unwrap(), 19);
                se.unwrap();
                sf.unwrap();
            })
            .await
            .expect("the selected route must retain its independent arms");
        });
    assert!(network.queue_is_empty());
}
