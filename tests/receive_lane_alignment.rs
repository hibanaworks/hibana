mod common;
#[path = "../examples/support/in_memory.rs"]
mod in_memory;

use core::{task::Poll, time::Duration};
use hibana::{
    Endpoint, EndpointError, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use in_memory::InMemoryTransport;

const OWNER: u8 = 0;
const WORKER: u8 = 1;
const SERVICE: u8 = 2;
const DB: u8 = 3;
type Start = g::Msg<1, ()>;
type Loan<'r> = g::Msg<2, Endpoint<'r, DB>>;
type Read = g::Msg<3, u32>;
type Value = g::Msg<4, u32>;
type Tick = g::Msg<5, ()>;
type Pong = g::Msg<6, ()>;
type Seal = g::Msg<7, ()>;
type Done = g::Msg<8, ()>;

#[test]
fn a_carrier_borrows_its_parallel_arm_before_the_resources_prefix() {
    type First = g::Msg<21, ()>;
    type Second = g::Msg<22, ()>;
    type Reply = g::Msg<23, ()>;
    type Other = g::Msg<24, ()>;
    type OtherReply = g::Msg<25, ()>;
    let global = g::seq(
        g::send::<OWNER, WORKER, Loan<'_>>(),
        g::seq(
            g::par(
                g::seq(
                    g::send::<DB, SERVICE, First>(),
                    g::seq(
                        g::send::<WORKER, SERVICE, Second>(),
                        g::send::<SERVICE, DB, Reply>(),
                    ),
                ),
                g::seq(
                    g::send::<DB, SERVICE, Other>(),
                    g::send::<SERVICE, DB, OtherReply>(),
                ),
            ),
            g::send::<WORKER, OWNER, Loan<'_>>(),
        ),
    );
    let p0 = project::<OWNER>(&global);
    let p1 = project::<WORKER>(&global);
    let p2 = project::<SERVICE>(&global);
    let p3 = project::<DB>(&global);
    let sid = SessionId::new(96);
    // SAFETY: this fixture owns one resource and creates its sole issuer.
    let network = InMemoryTransport::new([((sid, DB), unsafe { EndpointBinding::new() })]);
    let [mut slab0, mut slab1, mut slab2] = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
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
            tokio::time::timeout(Duration::from_secs(1), async {
                tokio::try_join!(
                    async {
                        owner.send::<Loan<'_>>(db).await?;
                        let _db = owner.recv::<Loan<'_>>().await?;
                        Ok::<_, EndpointError>(())
                    },
                    async {
                        assert!(
                            worker.par::<1>().is_err(),
                            "the carrier must first receive its loan"
                        );
                        let mut db = worker.recv::<Loan<'_>>().await?;
                        {
                            let [mut carrier] = worker
                                .par()
                                .expect("carrier arm is borrowable before the DB prefix");
                            let [mut left, mut right] = db
                                .par()
                                .expect("both DB arms are borrowable with the carrier arm");
                            let second = carrier.send::<Second>(&());
                            tokio::pin!(second);
                            assert!(
                                core::future::poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx)))
                                    .await
                                    .is_pending(),
                                "borrowing must not discharge the DB prefix"
                            );
                            tokio::try_join!(
                                second,
                                async {
                                    left.send::<First>(&()).await?;
                                    left.recv::<Reply>().await?;
                                    Ok::<_, EndpointError>(())
                                },
                                async {
                                    right.send::<Other>(&()).await?;
                                    right.recv::<OtherReply>().await?;
                                    Ok::<_, EndpointError>(())
                                },
                            )?;
                        }
                        worker.send::<Loan<'_>>(db).await?;
                        Ok::<_, EndpointError>(())
                    },
                    async {
                        let [mut left, mut right] = service.par()?;
                        tokio::try_join!(
                            async {
                                left.recv::<First>().await?;
                                left.recv::<Second>().await?;
                                left.send::<Reply>(&()).await?;
                                Ok::<_, EndpointError>(())
                            },
                            async {
                                right.recv::<Other>().await?;
                                right.send::<OtherReply>(&()).await?;
                                Ok::<_, EndpointError>(())
                            },
                        )?;
                        Ok::<_, EndpointError>(())
                    },
                )
            })
            .await
            .expect("both arms complete concurrently")
            .unwrap();
        });
}

#[test]
fn delegated_parallel_rolls_keep_their_cross_lane_continuation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let work = g::par(
                g::seq(
                    g::send::<DB, SERVICE, Read>(),
                    g::send::<SERVICE, DB, Value>(),
                )
                .roll()
                .roll(),
                g::seq(
                    g::send::<WORKER, OWNER, Tick>(),
                    g::send::<OWNER, WORKER, Pong>(),
                )
                .roll(),
            );
            let global = g::seq(
                g::send::<OWNER, WORKER, Start>(),
                g::seq(
                    g::send::<OWNER, WORKER, Loan<'_>>(),
                    g::seq(
                        work,
                        g::seq(
                            g::send::<WORKER, OWNER, Loan<'_>>(),
                            g::seq(
                                g::send::<DB, SERVICE, Seal>(),
                                g::send::<OWNER, SERVICE, Done>(),
                            ),
                        ),
                    ),
                ),
            );
            let p0 = project::<OWNER>(&global);
            let p1 = project::<WORKER>(&global);
            let p2 = project::<SERVICE>(&global);
            let p3 = project::<DB>(&global);
            for target in [OWNER, DB] {
                let sid = SessionId::new(81 + u32::from(target));
                // SAFETY: this fixture owns one resource and creates its sole issuer.
                let network =
                    InMemoryTransport::new([((sid, DB), unsafe { EndpointBinding::new() })]);
                let [mut slab0, mut slab1, mut slab2] = [
                    vec![0; 128 * 1024],
                    vec![0; 128 * 1024],
                    vec![0; 128 * 1024],
                ];
                let mut s0 = SessionKitStorage::uninit();
                let mut s1 = SessionKitStorage::uninit();
                let mut s2 = SessionKitStorage::uninit();
                let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
                let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
                let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
                let mut owner = rv0.enter(sid, &p0).unwrap();
                let mut worker = rv1.enter(sid, &p1).unwrap();
                let mut service = rv2.enter(sid, &p2).unwrap();
                let db = rv0.enter(sid, &p3).unwrap();
                tokio::time::timeout(Duration::from_secs(1), async {
                    tokio::try_join!(
                        async {
                            owner.send::<Start>(&()).await?;
                            owner.send::<Loan<'_>>(db).await?;
                            {
                                let [mut link] = owner.par()?;
                                for _ in 0..4 {
                                    link.recv::<Tick>().await?;
                                    link.send::<Pong>(&()).await?;
                                }
                            }
                            let mut db = owner.recv::<Loan<'_>>().await?;
                            db.send::<Seal>(&()).await?;
                            owner.send::<Done>(&()).await?;
                            let result = core::future::poll_fn(|cx| {
                                let result = if target == OWNER {
                                    let future = owner.send::<Start>(&());
                                    tokio::pin!(future);
                                    future.as_mut().poll(cx)
                                } else {
                                    let future = db.send::<Seal>(&());
                                    tokio::pin!(future);
                                    future.as_mut().poll(cx)
                                };
                                Poll::Ready(result)
                            })
                            .await;
                            assert!(
                                matches!(result, Poll::Ready(Err(_))),
                                "replayed target {target}: {result:?}"
                            );
                            Ok::<_, EndpointError>(())
                        },
                        async {
                            worker.recv::<Start>().await?;
                            let mut db = worker.recv::<Loan<'_>>().await?;
                            {
                                let [mut ticks] = worker.par()?;
                                let [mut requests] = db.par()?;
                                tokio::try_join!(
                                    async {
                                        for _ in 0..4 {
                                            ticks.send::<Tick>(&()).await?;
                                            ticks.recv::<Pong>().await?;
                                        }
                                        Ok::<_, EndpointError>(())
                                    },
                                    async {
                                        for number in 0..4 {
                                            requests.send::<Read>(&number).await?;
                                            assert_eq!(requests.recv::<Value>().await?, number + 1);
                                        }
                                        Ok::<_, EndpointError>(())
                                    }
                                )?;
                            }
                            worker.send::<Loan<'_>>(db).await?;
                            Ok::<_, EndpointError>(())
                        },
                        async {
                            {
                                let [mut requests] = service.par()?;
                                for number in 0..4 {
                                    assert_eq!(requests.recv::<Read>().await?, number);
                                    requests.send::<Value>(&(number + 1)).await?;
                                }
                            }
                            service.recv::<Seal>().await?;
                            service.recv::<Done>().await?;
                            Ok::<_, EndpointError>(())
                        },
                    )
                })
                .await
                .expect("legal parallel execution stalled")
                .unwrap();
            }
        });
}

#[test]
fn sequential_receives_keep_source_order_when_the_later_sender_runs_first() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type First = g::Msg<20, u32>;
            type Second = g::Msg<21, u32>;
            let global = g::seq(g::send::<0, 2, First>(), g::send::<1, 2, Second>());
            assert_eq!(g::diagnose(&global), None);
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            // The early receive is tried on a fresh run: its error must not hide a
            // failure in the positive execution, which uses the other run.
            for reject_early in [true, false] {
                let network = common::TestTransport::new();
                let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
                let mut s0 = SessionKitStorage::uninit();
                let mut s1 = SessionKitStorage::uninit();
                let mut s2 = SessionKitStorage::uninit();
                let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
                let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
                let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
                let sid = SessionId::new(100);
                let mut first = rv0.enter(sid, &p0).unwrap();
                let mut second = rv1.enter(sid, &p1).unwrap();
                let mut receiver = rv2.enter(sid, &p2).unwrap();
                tokio::time::timeout(Duration::from_secs(1), async {
                    // Sender 1 does not participate in the first communication and
                    // may enqueue its own message before sender 0 runs.
                    second.send::<Second>(&22).await.unwrap();
                    if reject_early {
                        let result = core::future::poll_fn(|cx| {
                            let future = receiver.recv::<Second>();
                            tokio::pin!(future);
                            Poll::Ready(future.as_mut().poll(cx))
                        })
                        .await;
                        assert!(
                            matches!(result, Poll::Ready(Err(_))),
                            "early receive: {result:?}"
                        );
                    } else {
                        tokio::try_join!(first.send::<First>(&11), async {
                            assert_eq!(receiver.recv::<First>().await?, 11);
                            assert_eq!(receiver.recv::<Second>().await?, 22);
                            Ok::<_, EndpointError>(())
                        })
                        .unwrap();
                        assert!(network.queue_is_empty());
                    }
                })
                .await
                .expect("cross-lane receive order stalled");
            }
        });
}

#[test]
fn a_holder_send_waits_for_its_other_view_to_complete_the_prefix() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type Loan = g::Msg<30, Endpoint<'static, DB>>;
            type First = g::Msg<31, ()>;
            type Second = g::Msg<32, ()>;
            let global = g::seq(
                g::send::<OWNER, WORKER, Loan>(),
                g::seq(
                    g::send::<DB, SERVICE, First>(),
                    g::seq(
                        g::send::<WORKER, SERVICE, Second>(),
                        g::send::<WORKER, OWNER, Loan>(),
                    ),
                ),
            );
            let p0 = project::<OWNER>(&global);
            let p1 = project::<WORKER>(&global);
            let p2 = project::<SERVICE>(&global);
            let p3 = project::<DB>(&global);
            {
                let network = common::TestTransport::new();
                let [mut slab0, mut slab1, mut slab2] = [[0; 32768]; 3];
                let mut s0 = SessionKitStorage::uninit();
                let mut s1 = SessionKitStorage::uninit();
                let mut s2 = SessionKitStorage::uninit();
                let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
                let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
                let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
                let sid = SessionId::new(101);
                let mut owner = rv0.enter(sid, &p0).unwrap();
                let mut worker = rv1.enter(sid, &p1).unwrap();
                let mut service = rv2.enter(sid, &p2).unwrap();
                let db = rv0.enter(sid, &p3).unwrap();
                tokio::time::timeout(Duration::from_secs(1), async {
                    owner.send::<Loan>(db).await.unwrap();
                    let mut db = worker.recv::<Loan>().await.unwrap();
                    {
                        let send = worker.send::<Second>(&());
                        tokio::pin!(send);
                        let result =
                            core::future::poll_fn(|cx| Poll::Ready(send.as_mut().poll(cx))).await;
                        assert!(
                            matches!(result, Poll::Pending),
                            "early holder send: {result:?}"
                        );
                        tokio::try_join!(db.send::<First>(&()), send.as_mut(), async {
                            service.recv::<First>().await?;
                            service.recv::<Second>().await?;
                            Ok::<_, EndpointError>(())
                        })
                        .unwrap();
                    }
                    worker.send::<Loan>(db).await.unwrap();
                    drop(owner.recv::<Loan>().await.unwrap());
                    assert!(network.queue_is_empty());
                })
                .await
                .expect("delegated prefix stalled");
            }
        });
}

#[test]
fn a_parallel_join_orders_receives_from_different_senders_on_every_visit() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type Request = g::Msg<40, u32>;
            type Work = g::Msg<41, u32>;
            type Reply = g::Msg<42, u32>;
            let global = g::seq(
                g::par(g::send::<0, 2, Request>(), g::send::<2, 1, Work>()),
                g::send::<1, 2, Reply>(),
            )
            .roll();
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
            let sid = SessionId::new(102);
            let mut producer = rv0.enter(sid, &p0).unwrap();
            let mut worker = rv1.enter(sid, &p1).unwrap();
            let mut collector = rv2.enter(sid, &p2).unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                tokio::try_join!(
                    async {
                        for number in 0..4 {
                            tokio::task::yield_now().await;
                            producer.send::<Request>(&number).await?;
                        }
                        Ok::<_, EndpointError>(())
                    },
                    async {
                        for number in 0..4 {
                            {
                                let [mut work] = worker.par()?;
                                assert_eq!(work.recv::<Work>().await?, number);
                            }
                            worker.send::<Reply>(&(number * 2)).await?;
                        }
                        Ok::<_, EndpointError>(())
                    },
                    async {
                        for number in 0..4 {
                            {
                                let [mut requests, mut outgoing] = collector.par()?;
                                let (_, received) = tokio::try_join!(biased;
                                    outgoing.send::<Work>(&number),
                                    requests.recv::<Request>()
                                )?;
                                assert_eq!(received, number);
                            }
                            assert_eq!(collector.recv::<Reply>().await?, number * 2);
                        }
                        Ok::<_, EndpointError>(())
                    }
                )
                .unwrap();
            })
            .await
            .expect("parallel receive join stalled");
            assert!(network.queue_is_empty());
        });
}

#[test]
fn equal_labels_on_distinct_directions_keep_the_rolled_body_and_continuation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type Value = g::Msg<5, u32>;
            let global = g::seq(g::send::<1, 0, Value>().roll(), g::send::<2, 0, Value>());
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
            let sid = SessionId::new(103);
            let mut receiver = rv0.enter(sid, &p0).unwrap();
            let mut repeated = rv1.enter(sid, &p1).unwrap();
            let mut continuation = rv2.enter(sid, &p2).unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                continuation.send::<Value>(&200).await.unwrap();
                repeated.send::<Value>(&11).await.unwrap();
                assert_eq!(receiver.recv::<Value>().await.unwrap(), 11);
                assert_eq!(receiver.recv::<Value>().await.unwrap(), 200);
                for value in [12, 13] {
                    repeated.send::<Value>(&value).await.unwrap();
                    assert_eq!(receiver.recv::<Value>().await.unwrap(), value);
                }
            })
            .await
            .expect("equal-label continuation stalled");
            assert!(network.queue_is_empty());
        });
}
