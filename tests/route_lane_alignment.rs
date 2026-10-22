mod common;
use core::cell::Cell;
use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
};

type NoticeA = g::Msg<0, ()>;
type NoticeB = g::Msg<1, ()>;
type NoticeC = g::Msg<2, ()>;
type LoanA = g::Msg<3, Endpoint<'static, 3>>;
type ReadA = g::Msg<4, u32>;
type ValueA = g::Msg<5, u32>;
type ReturnA = g::Msg<6, Endpoint<'static, 3>>;
type LoanB = g::Msg<7, Endpoint<'static, 3>>;
type ReadB = g::Msg<8, u32>;
type ValueB = g::Msg<9, u32>;
type ReturnB = g::Msg<10, Endpoint<'static, 3>>;

#[test]
fn an_unselected_route_entry_cannot_consume_a_queued_continuation() {
    type M0 = g::Msg<0, ()>;
    type M1 = g::Msg<1, ()>;
    type M2 = g::Msg<2, ()>;
    type M3 = g::Msg<3, ()>;
    type M4 = g::Msg<4, ()>;
    type M5 = g::Msg<5, ()>;
    type M6 = g::Msg<6, ()>;
    type M7 = g::Msg<7, ()>;
    type M8 = g::Msg<8, ()>;
    type M9 = g::Msg<9, ()>;
    type M10 = g::Msg<10, ()>;
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, M0>(),
                g::seq(g::send::<0, 2, M1>(), g::send::<1, 2, M2>()),
            ),
            g::seq(
                g::send::<0, 1, M3>(),
                g::seq(
                    g::send::<1, 2, M4>(),
                    g::seq(g::send::<2, 1, M5>(), g::send::<1, 0, M6>()),
                ),
            ),
        ),
        g::seq(
            g::send::<0, 1, M7>(),
            g::seq(
                g::send::<1, 2, M8>(),
                g::seq(g::send::<2, 1, M9>(), g::send::<1, 0, M10>()),
            ),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(1814);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let _c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            a.send::<M0>(&()).await.unwrap();
            a.send::<M1>(&()).await.unwrap();
            a.send::<M7>(&()).await.unwrap();
            let (entry, _) = network.queued_frame_at(1, 0);
            let (continuation, _) = network.queued_frame_at(1, 1);
            assert_ne!(&entry.bytes()[7..], &continuation.bytes()[7..]);
            let future = b.recv::<M3>();
            tokio::pin!(future);
            let outcome =
                core::future::poll_fn(|cx| core::task::Poll::Ready(future.as_mut().poll(cx))).await;
            assert!(
                !matches!(outcome, core::task::Poll::Ready(Ok(()))),
                "a frame from the continuation was accepted as the unselected entry: {outcome:?}"
            );
        });
}

#[test]
fn an_early_continuation_does_not_hide_the_selected_route_entry() {
    type M0 = g::Msg<0, ()>;
    type M1 = g::Msg<1, ()>;
    type M2 = g::Msg<2, ()>;
    type M3 = g::Msg<3, ()>;
    type M4 = g::Msg<4, ()>;
    type M5 = g::Msg<5, ()>;
    type M6 = g::Msg<6, ()>;
    type M7 = g::Msg<7, ()>;
    type M8 = g::Msg<8, ()>;
    type M9 = g::Msg<9, ()>;
    type M10 = g::Msg<10, ()>;
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, M0>(),
                g::seq(g::send::<0, 2, M1>(), g::send::<1, 2, M2>()),
            ),
            g::seq(
                g::send::<0, 1, M3>(),
                g::seq(
                    g::send::<1, 2, M4>(),
                    g::seq(g::send::<2, 1, M5>(), g::send::<1, 0, M6>()),
                ),
            ),
        ),
        g::seq(
            g::send::<0, 1, M7>(),
            g::seq(
                g::send::<1, 2, M8>(),
                g::seq(g::send::<2, 1, M9>(), g::send::<1, 0, M10>()),
            ),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(1814);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            a.send::<M0>(&()).await.unwrap();
            a.send::<M1>(&()).await.unwrap();
            a.send::<M7>(&()).await.unwrap();
            let (entry, _) = network.queued_frame_at(1, 0);
            let (continuation, _) = network.queued_frame_at(1, 1);
            assert_ne!(&entry.bytes()[7..], &continuation.bytes()[7..]);
            let branch = b.offer().await.unwrap();
            assert_eq!(branch.label(), 0);
            branch.recv::<M0>().await.unwrap();
            c.recv::<M1>().await.unwrap();
            b.send::<M2>(&()).await.unwrap();
            c.recv::<M2>().await.unwrap();
            b.recv::<M7>().await.unwrap();
            b.send::<M8>(&()).await.unwrap();
            c.recv::<M8>().await.unwrap();
            c.send::<M9>(&()).await.unwrap();
            b.recv::<M9>().await.unwrap();
            b.send::<M10>(&()).await.unwrap();
            a.recv::<M10>().await.unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_choice_inside_parallel_can_offer_a_four_way_arm() {
    type Outside = g::Msg<80, ()>;
    type First = g::Msg<81, ()>;
    type Second = g::Msg<82, ()>;
    type Third = g::Msg<83, ()>;
    let global = g::par(
        g::send::<0, 1, Outside>(),
        g::route(
            g::par(
                g::par(
                    g::par(g::send::<0, 1, First>(), g::send::<0, 1, Second>()),
                    g::send::<0, 1, Third>(),
                ),
                g::seq(
                    g::send::<0, 1, LoanA>(),
                    g::seq(
                        g::send::<3, 2, ReadA>(),
                        g::seq(g::send::<2, 3, ValueA>(), g::send::<1, 0, ReturnA>()),
                    ),
                ),
            ),
            g::seq(
                g::send::<0, 1, LoanB>(),
                g::seq(
                    g::send::<3, 2, ReadB>(),
                    g::seq(g::send::<2, 3, ValueB>(), g::send::<1, 0, ReturnB>()),
                ),
            ),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(9312);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let db = rv0.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                {
                    use core::{
                        future::Future as _,
                        task::{Context, Poll, Waker},
                    };
                    let mut waiting = core::pin::pin!(worker.offer());
                    let mut cx = Context::from_waker(Waker::noop());
                    assert!(matches!(waiting.as_mut().poll(&mut cx), Poll::Pending));
                }
                owner.send::<First>(&()).await?;
                let branch = worker.offer().await?;
                assert_eq!(branch.label(), 81);
                drop(branch);
                worker.offer().await?.recv::<First>().await?;
                tokio::try_join!(owner.send::<Second>(&()), worker.recv::<Second>())?;
                tokio::try_join!(owner.send::<Third>(&()), worker.recv::<Third>())?;
                let (_returned, (), ()) = tokio::try_join!(
                    async {
                        owner.send::<LoanA>(db).await?;
                        owner.recv::<ReturnA>().await
                    },
                    async {
                        let mut db = worker.recv::<LoanA>().await?;
                        db.send::<ReadA>(&41).await?;
                        assert_eq!(db.recv::<ValueA>().await?, 42);
                        worker.send::<ReturnA>(db).await
                    },
                    async {
                        let number = service.recv::<ReadA>().await?;
                        service.send::<ValueA>(&(number + 1)).await
                    },
                )?;
                tokio::try_join!(owner.send::<Outside>(&()), worker.recv::<Outside>())?;
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("the selected parallel arm must complete")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_completed_parallel_exchange_keeps_the_nested_loan_choice() {
    type UnusedLoan = g::Msg<85, Endpoint<'static, 3>>;
    type UnusedRead = g::Msg<86, ()>;
    type UnusedValue = g::Msg<87, ()>;
    type UnusedReturn = g::Msg<88, Endpoint<'static, 3>>;
    type OtherLoan = g::Msg<90, Endpoint<'static, 3>>;
    type OtherRead = g::Msg<91, ()>;
    type OtherValue = g::Msg<92, ()>;
    type OtherReturn = g::Msg<93, Endpoint<'static, 3>>;
    type Tick = g::Msg<94, ()>;
    type Notice = g::Msg<11, ()>;
    type Request = g::Msg<12, ()>;
    type Reply = g::Msg<13, ()>;
    type Ack = g::Msg<14, ()>;
    let global = g::route(
        g::route(
            g::seq(
                g::send::<0, 1, UnusedLoan>(),
                g::seq(
                    g::send::<3, 2, UnusedRead>(),
                    g::seq(
                        g::send::<2, 3, UnusedValue>(),
                        g::send::<1, 0, UnusedReturn>(),
                    ),
                ),
            ),
            g::par(
                g::route(
                    g::seq(
                        g::send::<0, 1, LoanA>(),
                        g::seq(
                            g::send::<3, 2, ReadA>(),
                            g::seq(g::send::<2, 3, ValueA>(), g::send::<1, 0, ReturnA>()),
                        ),
                    ),
                    g::seq(
                        g::send::<0, 1, LoanB>(),
                        g::seq(
                            g::send::<3, 2, ReadB>(),
                            g::seq(g::send::<2, 3, ValueB>(), g::send::<1, 0, ReturnB>()),
                        ),
                    ),
                ),
                g::seq(
                    g::send::<0, 1, Notice>(),
                    g::seq(
                        g::send::<1, 2, Request>(),
                        g::seq(g::send::<2, 1, Reply>(), g::send::<1, 0, Ack>()),
                    ),
                ),
            ),
        ),
        g::par(
            g::seq(
                g::send::<0, 1, OtherLoan>(),
                g::seq(
                    g::send::<3, 2, OtherRead>(),
                    g::seq(
                        g::send::<2, 3, OtherValue>(),
                        g::send::<1, 0, OtherReturn>(),
                    ),
                ),
            ),
            g::send::<0, 2, Tick>(),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(9313);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let db = rv0.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                owner.send::<Notice>(&()).await?;
                worker.offer().await?.recv::<Notice>().await?;
                worker.send::<Request>(&()).await?;
                service.recv::<Request>().await?;
                service.send::<Reply>(&()).await?;
                worker.recv::<Reply>().await?;
                worker.send::<Ack>(&()).await?;
                owner.recv::<Ack>().await?;
                owner.send::<LoanA>(db).await?;
                let branch = worker
                    .offer()
                    .await
                    .expect("independent nested loan choice");
                assert_eq!(branch.label(), 3);
                drop(branch);
                let mut db = worker.offer().await?.recv::<LoanA>().await?;
                db.send::<ReadA>(&41).await?;
                let number = service.recv::<ReadA>().await?;
                service.send::<ValueA>(&(number + 1)).await?;
                assert_eq!(db.recv::<ValueA>().await?, 42);
                worker.send::<ReturnA>(db).await?;
                let _returned = owner.recv::<ReturnA>().await?;
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("the independent nested choice must complete")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_later_visit_notice_can_arrive_before_the_earlier_ack() {
    use core::{
        future::Future as _,
        task::{Context, Poll, Waker},
    };
    type NL = g::Msg<10, u32>;
    type NR = g::Msg<11, u32>;
    type DL = g::Msg<12, u32>;
    type DR = g::Msg<13, u32>;
    type Ack = g::Msg<14, u32>;
    type Tail = g::Msg<15, u32>;
    type Ready = g::Msg<17, u32>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<2, 0, NL>(), g::send::<0, 1, DL>()),
            g::seq(
                g::send::<2, 0, NR>(),
                g::seq(
                    g::send::<0, 2, Ready>(),
                    g::seq(g::send::<2, 1, DR>(), g::send::<1, 0, Ack>()),
                ),
            ),
        ),
        g::send::<0, 1, Tail>(),
    )
    .roll();
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(9293);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            c.send::<NR>(&1).await.unwrap();
            a.recv::<NR>().await.unwrap();
            a.send::<Ready>(&2).await.unwrap();
            c.recv::<Ready>().await.unwrap();
            c.send::<DR>(&3).await.unwrap();
            b.recv::<DR>().await.unwrap();
            c.send::<NL>(&4).await.unwrap();
            {
                let mut ack = core::pin::pin!(a.recv::<Ack>());
                let mut cx = Context::from_waker(Waker::noop());
                assert!(
                    matches!(ack.as_mut().poll(&mut cx), Poll::Pending),
                    "the next notice must not satisfy the earlier ACK"
                );
            }
            b.send::<Ack>(&5).await.unwrap();
            assert_eq!(a.recv::<Ack>().await.unwrap(), 5);
            a.send::<Tail>(&6).await.unwrap();
            assert_eq!(b.recv::<Tail>().await.unwrap(), 6);
            assert_eq!(a.recv::<NL>().await.unwrap(), 4);
            a.send::<DL>(&7).await.unwrap();
            assert_eq!(b.recv::<DL>().await.unwrap(), 7);
            a.send::<Tail>(&8).await.unwrap();
            assert_eq!(b.recv::<Tail>().await.unwrap(), 8);
        });
    assert!(network.queue_is_empty());
}

#[test]
fn an_early_continuation_cannot_impersonate_another_choice_arm() {
    use core::{
        future::Future as _,
        task::{Context, Poll, Waker},
    };
    type NoticeLeft = g::Msg<10, u32>;
    type NoticeRight = g::Msg<11, u32>;
    type DataLeft = g::Msg<12, u32>;
    type DataRight = g::Msg<13, u32>;
    type Tail = g::Msg<15, u32>;
    let global = g::seq(
        g::route(
            g::seq(g::send::<2, 0, NoticeLeft>(), g::send::<0, 1, DataLeft>()),
            g::seq(g::send::<2, 0, NoticeRight>(), g::send::<2, 1, DataRight>()),
        ),
        g::send::<0, 1, Tail>(),
    )
    .roll();
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    let sid = SessionId::new(9292);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            c.send::<NoticeRight>(&1).await.unwrap();
            assert_eq!(a.recv::<NoticeRight>().await.unwrap(), 1);
            a.send::<Tail>(&99).await.unwrap();
            {
                let mut offer = core::pin::pin!(b.offer());
                let mut cx = Context::from_waker(Waker::noop());
                match offer.as_mut().poll(&mut cx) {
                    Poll::Pending => (),
                    Poll::Ready(Ok(branch)) => {
                        panic!("the early tail selected branch {}", branch.label())
                    }
                    Poll::Ready(Err(error)) => {
                        panic!("the early tail faulted a legal choice: {error:?}")
                    }
                }
            }
            c.send::<DataRight>(&7).await.unwrap();
            assert_eq!(b.recv::<DataRight>().await.unwrap(), 7);
            assert_eq!(b.recv::<Tail>().await.unwrap(), 99);
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_received_endpoint_choice_joins_before_its_return_on_another_lane() {
    type Loan = g::Msg<0, Endpoint<'static, 3>>;
    type Left = g::Msg<1, ()>;
    type LeftNotice = g::Msg<2, ()>;
    type ReadLeft = g::Msg<3, ()>;
    type ReplyLeft = g::Msg<4, ()>;
    type Right = g::Msg<5, ()>;
    type RightNotice = g::Msg<6, ()>;
    type ReadRight = g::Msg<7, ()>;
    type ReplyRight = g::Msg<8, ()>;
    type Return = g::Msg<9, Endpoint<'static, 3>>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(
                g::seq(
                    g::send::<3, 2, Left>(),
                    g::seq(
                        g::send::<3, 0, LeftNotice>(),
                        g::seq(g::send::<3, 2, ReadLeft>(), g::send::<2, 3, ReplyLeft>()),
                    ),
                ),
                g::seq(
                    g::send::<3, 2, Right>(),
                    g::seq(
                        g::send::<3, 0, RightNotice>(),
                        g::seq(g::send::<3, 2, ReadRight>(), g::send::<2, 3, ReplyRight>()),
                    ),
                ),
            )
            .resolve::<0>(),
            g::send::<1, 0, Return>(),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    for choice in [DecisionArm::Left, DecisionArm::Right] {
        let network = common::TestTransport::new();
        let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let mut s2 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
        rv1.set_resolver(
            &p3,
            ResolverRef::<0>::decision_state(&choice, |arm| Ok(*arm)),
        )
        .unwrap();
        let sid = SessionId::new(9291);
        let mut owner = rv0.enter(sid, &p0).unwrap();
        let db = rv0.enter(sid, &p3).unwrap();
        let mut worker = rv1.enter(sid, &p1).unwrap();
        let mut service = rv2.enter(sid, &p2).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(core::time::Duration::from_secs(1), async {
                    let owner_run = async {
                        owner.send::<Loan>(db).await?;
                        let branch = owner.offer().await?;
                        match branch.label() {
                            2 => branch.recv::<LeftNotice>().await?,
                            6 => branch.recv::<RightNotice>().await?,
                            _ => panic!("unexpected notice"),
                        };
                        drop(owner.recv::<Return>().await?);
                        Ok::<_, hibana::EndpointError>(())
                    };
                    let worker_run = async {
                        let mut db = worker.recv::<Loan>().await?;
                        let branch = db.offer().await?;
                        match branch.label() {
                            1 => {
                                branch.send::<Left>(&()).await?;
                                db.send::<LeftNotice>(&()).await?;
                                db.send::<ReadLeft>(&()).await?;
                                db.recv::<ReplyLeft>().await?;
                            }
                            5 => {
                                branch.send::<Right>(&()).await?;
                                db.send::<RightNotice>(&()).await?;
                                db.send::<ReadRight>(&()).await?;
                                db.recv::<ReplyRight>().await?;
                            }
                            _ => panic!("unexpected delegated choice"),
                        }
                        worker.send::<Return>(db).await
                    };
                    let service_run = async {
                        let branch = service.offer().await?;
                        match branch.label() {
                            1 => {
                                branch.recv::<Left>().await?;
                                service.recv::<ReadLeft>().await?;
                                service.send::<ReplyLeft>(&()).await
                            }
                            5 => {
                                branch.recv::<Right>().await?;
                                service.recv::<ReadRight>().await?;
                                service.send::<ReplyRight>(&()).await
                            }
                            _ => panic!("unexpected service choice"),
                        }
                    };
                    tokio::try_join!(owner_run, worker_run, service_run)
                })
                .await
                .expect("delegated choice stalled")
                .unwrap();
            });
        assert!(network.queue_is_empty());
    }
}

fn global() -> impl hibana::runtime::program::Projectable {
    g::route(
        g::par(
            g::route(g::send::<1, 2, NoticeA>(), g::send::<1, 2, NoticeB>()).resolve::<0>(),
            g::seq(
                g::send::<1, 0, LoanA>(),
                g::seq(
                    g::send::<3, 2, ReadA>(),
                    g::seq(g::send::<2, 3, ValueA>(), g::send::<0, 1, ReturnA>()),
                ),
            ),
        ),
        g::seq(
            g::send::<1, 0, LoanB>(),
            g::seq(
                g::send::<3, 2, ReadB>(),
                g::seq(g::send::<2, 3, ValueB>(), g::send::<0, 1, ReturnB>()),
            ),
        ),
    )
    .resolve::<1>()
    .roll()
}

#[test]
fn compound_parallel_choice_keeps_its_receive_directions_across_visits() {
    let global = global();
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let outer = Cell::new(DecisionArm::Left);
    let inner = Cell::new(DecisionArm::Left);
    let outer_calls = Cell::new(0);
    let inner_calls = Cell::new(0);
    let outer_input = (&outer, &outer_calls);
    let inner_input = (&inner, &inner_calls);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let rv0 = storage0
        .init()
        .rendezvous(&mut slab0, network.clone())
        .unwrap();
    let rv1 = storage1
        .init()
        .rendezvous(&mut slab1, network.clone())
        .unwrap();
    let rv2 = storage2
        .init()
        .rendezvous(&mut slab2, network.clone())
        .unwrap();
    rv1.set_resolver(
        &p1,
        ResolverRef::<0>::decision_state(&inner_input, |input| {
            input.1.set(input.1.get() + 1);
            Ok(input.0.get())
        }),
    )
    .unwrap();
    rv1.set_resolver(
        &p1,
        ResolverRef::<1>::decision_state(&outer_input, |input| {
            input.1.set(input.1.get() + 1);
            Ok(input.0.get())
        }),
    )
    .unwrap();
    let sid = SessionId::new(9289);
    let mut owner = rv1.enter(sid, &p1).unwrap();
    let mut worker = rv0.enter(sid, &p0).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let mut db = rv1.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for (number, (choice, notice)) in [
                    (DecisionArm::Left, DecisionArm::Left),
                    (DecisionArm::Left, DecisionArm::Right),
                    (DecisionArm::Right, DecisionArm::Left),
                    (DecisionArm::Left, DecisionArm::Left),
                    (DecisionArm::Left, DecisionArm::Right),
                    (DecisionArm::Right, DecisionArm::Right),
                    (DecisionArm::Left, DecisionArm::Left),
                ]
                .into_iter()
                .enumerate()
                {
                    outer.set(choice);
                    inner.set(notice);
                    let owner_run = async {
                        let branch = owner.offer().await?;
                        let label = branch.label();
                        drop(branch);
                        if label == 7 {
                            owner.send::<LoanB>(db).await?;
                            owner.recv::<ReturnB>().await
                        } else {
                            let [mut notices, mut loans] = owner.par()?;
                            let loan = async {
                                loans.send::<LoanA>(db).await?;
                                loans.recv::<ReturnA>().await
                            };
                            let notification = async {
                                // Delay delivery, so service observes the
                                // query before the independent notice.
                                tokio::task::yield_now().await;
                                let branch = notices.offer().await?;
                                match branch.label() {
                                    0 => branch.send::<NoticeA>(&()).await,
                                    1 => branch.send::<NoticeB>(&()).await,
                                    _ => panic!("unexpected notice"),
                                }
                            };
                            let (returned, ()) = tokio::try_join!(biased; loan, notification)?;
                            Ok::<_, hibana::EndpointError>(returned)
                        }
                    };
                    let worker_run = async {
                        let branch = worker.offer().await?;
                        match branch.label() {
                            3 => {
                                let mut db = branch.recv::<LoanA>().await?;
                                db.send::<ReadA>(&(number as u32)).await?;
                                assert_eq!(db.recv::<ValueA>().await?, number as u32 + 1);
                                worker.send::<ReturnA>(db).await
                            }
                            7 => {
                                let mut db = branch.recv::<LoanB>().await?;
                                db.send::<ReadB>(&(number as u32)).await?;
                                assert_eq!(db.recv::<ValueB>().await?, number as u32 + 1);
                                worker.send::<ReturnB>(db).await
                            }
                            _ => panic!("unexpected loan"),
                        }
                    };
                    let service_run = async {
                        let branch = service.offer().await?;
                        let label = branch.label();
                        drop(branch);
                        if label == 8 {
                            let value = service.offer().await?.recv::<ReadB>().await?;
                            service.send::<ValueB>(&(value + 1)).await
                        } else {
                            assert_eq!(label, 4, "the query should arrive before the notice");
                            let [mut notices, mut queries] = service.par()?;
                            let notification = async {
                                let branch = notices.offer().await?;
                                match branch.label() {
                                    0 => branch.recv::<NoticeA>().await,
                                    1 => branch.recv::<NoticeB>().await,
                                    _ => panic!("unexpected notice"),
                                }
                            };
                            let query = async {
                                let value = queries.recv::<ReadA>().await?;
                                queries.send::<ValueA>(&(value + 1)).await
                            };
                            tokio::try_join!(biased; notification, query)?;
                            Ok::<_, hibana::EndpointError>(())
                        }
                    };
                    let (returned, (), ()) =
                        tokio::try_join!(biased; owner_run, worker_run, service_run)?;
                    db = returned;
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("route lane alignment stalled")
            .unwrap();
        });
    assert_eq!(outer_calls.get(), 7);
    assert_eq!(inner_calls.get(), 5);
    assert!(network.queue_is_empty());
}

#[test]
fn a_later_parallel_notice_can_choose_the_same_arm_on_the_next_visit() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                let global = g::route(
                    g::par(
                        g::route(
                            g::send::<1, 2, NoticeA>(),
                            g::route(g::send::<1, 2, NoticeB>(), g::send::<1, 2, NoticeC>()),
                        )
                        .resolve::<0>(),
                        g::seq(
                            g::send::<1, 0, LoanA>(),
                            g::seq(
                                g::send::<3, 2, ReadA>(),
                                g::seq(g::send::<2, 3, ValueA>(), g::send::<0, 1, ReturnA>()),
                            ),
                        ),
                    ),
                    g::seq(
                        g::send::<1, 0, LoanB>(),
                        g::seq(
                            g::send::<3, 2, ReadB>(),
                            g::seq(g::send::<2, 3, ValueB>(), g::send::<0, 1, ReturnB>()),
                        ),
                    ),
                )
                .roll();
                let p0 = project::<0>(&global);
                let p1 = project::<1>(&global);
                let p2 = project::<2>(&global);
                let p3 = project::<3>(&global);
                let network = common::TestTransport::new();
                let calls = Cell::new(0);
                let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
                let mut storage0 = SessionKitStorage::uninit();
                let mut storage1 = SessionKitStorage::uninit();
                let mut storage2 = SessionKitStorage::uninit();
                let rv0 = storage0
                    .init()
                    .rendezvous(&mut slab0, network.clone())
                    .unwrap();
                let rv1 = storage1
                    .init()
                    .rendezvous(&mut slab1, network.clone())
                    .unwrap();
                let rv2 = storage2
                    .init()
                    .rendezvous(&mut slab2, network.clone())
                    .unwrap();
                rv1.set_resolver(
                    &p1,
                    ResolverRef::<0>::decision_state(&calls, |calls| {
                        calls.set(calls.get() + 1);
                        Ok(DecisionArm::Left)
                    }),
                )
                .unwrap();
                let sid = SessionId::new(9290);
                let mut owner = rv1.enter(sid, &p1).unwrap();
                let mut worker = rv0.enter(sid, &p0).unwrap();
                let mut service = rv2.enter(sid, &p2).unwrap();
                let db = rv1.enter(sid, &p3).unwrap();
                owner.send::<NoticeA>(&()).await.unwrap();
                service
                    .offer()
                    .await
                    .unwrap()
                    .recv::<NoticeA>()
                    .await
                    .unwrap();
                owner.send::<LoanA>(db).await.unwrap();
                let mut db = worker.recv::<LoanA>().await.unwrap();
                db.send::<ReadA>(&1).await.unwrap();
                assert_eq!(service.recv::<ReadA>().await.unwrap(), 1);
                service.send::<ValueA>(&2).await.unwrap();
                assert_eq!(db.recv::<ValueA>().await.unwrap(), 2);
                worker.send::<ReturnA>(db).await.unwrap();
                let db = owner.recv::<ReturnA>().await.unwrap();

                owner.send::<LoanA>(db).await.unwrap();
                let mut db = worker.offer().await.unwrap().recv::<LoanA>().await.unwrap();
                db.send::<ReadA>(&3).await.unwrap();
                assert_eq!(service.recv::<ReadA>().await.unwrap(), 3);
                service.send::<ValueA>(&4).await.unwrap();
                assert_eq!(db.recv::<ValueA>().await.unwrap(), 4);
                worker.send::<ReturnA>(db).await.unwrap();
                let _db = owner.recv::<ReturnA>().await.unwrap();
                owner.send::<NoticeA>(&()).await.unwrap();
                service
                    .offer()
                    .await
                    .unwrap()
                    .recv::<NoticeA>()
                    .await
                    .unwrap();
                assert_eq!(calls.get(), 2);
                assert!(network.queue_is_empty());
            })
            .await
            .expect("late parallel notice stalled");
        });
}
