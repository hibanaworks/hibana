use crate::test_support::transport::TestTransport;
use crate::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[path = "handoff_tests/delegated_resolver.rs"]
mod delegated_resolver;
#[path = "handoff_tests/parallel_rolls.rs"]
mod parallel_rolls;

#[test]
fn incoming_choice_can_arrive_at_either_of_the_actors_views() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for right in [false, true] {
                for db_first in [false, true] {
                    type Left = g::Msg<0, ()>;
                    type Right = g::Msg<1, ()>;
                    type Loan<'r> = g::Msg<2, Endpoint<'r, 3>>;
                    type Tail = g::Msg<3, ()>;
                    let global = g::seq(
                        g::route(g::send::<0, 1, Left>(), g::send::<0, 3, Right>()),
                        g::seq(g::send::<1, 2, Loan<'_>>(), g::send::<0, 3, Tail>()),
                    );
                    let p0 = project::<0>(&global);
                    let p1 = project::<1>(&global);
                    let p2 = project::<2>(&global);
                    let p3 = project::<3>(&global);
                    let id = SessionId::new(281);
                    let network = TestTransport::new();
                    let mut slab0 = [0; 65536];
                    let mut slab1 = [0; 65536];
                    let mut slab2 = [0; 65536];
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
                    let mut sender = rv0.enter(id, &p0).unwrap();
                    let mut worker = rv1.enter(id, &p1).unwrap();
                    let mut next = rv2.enter(id, &p2).unwrap();
                    let mut db = rv1.enter(id, &p3).unwrap();
                    tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::try_join!(
                    biased;
                    async {
                        if right {
                            sender.send::<Right>(&()).await?;
                        } else {
                            sender.send::<Left>(&()).await?;
                        }
                        sender.send::<Tail>(&()).await
                    },
                    async {
                        let label = if db_first {
                            tokio::select! {
                                biased;
                                branch = db.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                                branch = worker.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                            }
                        } else {
                            tokio::select! {
                                biased;
                                branch = worker.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                                branch = db.offer() => {
                                    let branch = branch?;
                                    branch.label()
                                },
                            }
                        };
                        assert_eq!(label, u8::from(right), "right={right} db_first={db_first}");
                        match label {
                            0 => {
                                let branch = worker.offer().await?;
                                branch.recv::<Left>().await?;
                            }
                            1 => {
                                let branch = db.offer().await?;
                                branch.recv::<Right>().await?;
                            }
                            _ => panic!("unexpected choice"),
                        }
                        worker.send::<Loan<'_>>(db).await
                    },
                    async {
                        let mut db = next.recv::<Loan<'_>>().await?;
                        db.recv::<Tail>().await
                    }
                )
            })
            .await
            .expect("choice did not progress")
            .unwrap_or_else(|error| panic!("right={right}, db_first={db_first}: {error:?}"));
                    assert!(network.queue_is_empty());
                }
            }
        });
}

#[test]
fn an_alternate_receive_is_polled_when_the_views_first_arm_starts_with_send() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for right in [false, true] {
                for db_first in [false, true] {
                    type LeftKnow = g::Msg<0, ()>;
                    type LeftReply = g::Msg<1, ()>;
                    type Right = g::Msg<2, ()>;
                    type Loan<'r> = g::Msg<3, Endpoint<'r, 3>>;
                    let global = g::seq(
                        g::route(
                            g::seq(g::send::<0, 3, LeftKnow>(), g::send::<1, 0, LeftReply>()),
                            g::send::<0, 1, Right>(),
                        ),
                        g::send::<1, 2, Loan<'_>>(),
                    );
                    let p0 = project::<0>(&global);
                    let p1 = project::<1>(&global);
                    let p2 = project::<2>(&global);
                    let p3 = project::<3>(&global);
                    let id = SessionId::new(282);
                    let network = TestTransport::new();
                    let mut slab0 = [0; 65536];
                    let mut slab1 = [0; 65536];
                    let mut slab2 = [0; 65536];
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
                    let mut sender = rv0.enter(id, &p0).unwrap();
                    let mut worker = rv1.enter(id, &p1).unwrap();
                    let mut next = rv2.enter(id, &p2).unwrap();
                    let mut db = rv1.enter(id, &p3).unwrap();
                    tokio::time::timeout(core::time::Duration::from_secs(1), async {
                        tokio::try_join!(
                            biased;
                            async {
                                if right {
                                    sender.send::<Right>(&()).await?;
                                } else {
                                    sender.send::<LeftKnow>(&()).await?;
                                    sender.recv::<LeftReply>().await?;
                                }
                                Ok(())
                            },
                            async {
                                let label = if db_first {
                                    tokio::select! {
                                        biased;
                                        branch = db.offer() => branch?.label(),
                                        branch = worker.offer() => branch?.label(),
                                    }
                                } else {
                                    tokio::select! {
                                        biased;
                                        branch = worker.offer() => branch?.label(),
                                        branch = db.offer() => branch?.label(),
                                    }
                                };
                                assert_eq!(
                                    label,
                                    if right { 2 } else { 0 },
                                    "right={right} db_first={db_first}"
                                );
                                match label {
                                    0 => {
                                        db.offer().await?.recv::<LeftKnow>().await?;
                                        let branch = worker.offer().await?;
                                        assert_eq!(branch.label(), 1);
                                        branch.send::<LeftReply>(&()).await?;
                                    }
                                    2 => {
                                        worker.offer().await?.recv::<Right>().await?;
                                    }
                                    _ => panic!("unexpected choice"),
                                }
                                worker.send::<Loan<'_>>(db).await
                            },
                            async {
                                let _db = next.recv::<Loan<'_>>().await?;
                                Ok(())
                            },
                        )
                    })
                    .await
                    .unwrap_or_else(|_| {
                        panic!("choice did not progress: right={right} db_first={db_first}")
                    })
                    .unwrap_or_else(|error| {
                        panic!("right={right}, db_first={db_first}: {error:?}")
                    });
                    assert!(network.queue_is_empty());
                }
            }
        });
}

#[test]
fn a_nested_alternate_receive_is_found_past_other_views_and_send_entries() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for choice in 0..3 {
                for worker_first in [false, true] {
                    type LeftKnow = g::Msg<0, ()>;
                    type LeftReply = g::Msg<1, ()>;
                    type InnerLeftKnow = g::Msg<2, ()>;
                    type InnerLeftReply = g::Msg<3, ()>;
                    type InnerRight = g::Msg<4, ()>;
                    type Loan3<'r> = g::Msg<5, Endpoint<'r, 3>>;
                    type Loan4<'r> = g::Msg<6, Endpoint<'r, 4>>;
                    let global = g::seq(
                        g::route(
                            g::seq(g::send::<0, 3, LeftKnow>(), g::send::<1, 0, LeftReply>()),
                            g::route(
                                g::seq(
                                    g::send::<0, 4, InnerLeftKnow>(),
                                    g::send::<1, 0, InnerLeftReply>(),
                                ),
                                g::send::<0, 1, InnerRight>(),
                            ),
                        ),
                        g::seq(g::send::<1, 2, Loan3<'_>>(), g::send::<1, 2, Loan4<'_>>()),
                    );
                    let p0 = project::<0>(&global);
                    let p1 = project::<1>(&global);
                    let p2 = project::<2>(&global);
                    let p3 = project::<3>(&global);
                    let p4 = project::<4>(&global);
                    let id = SessionId::new(283);
                    let network = TestTransport::new();
                    let mut slab0 = [0; 65536];
                    let mut slab1 = [0; 65536];
                    let mut slab2 = [0; 65536];
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
                    let mut sender = rv0.enter(id, &p0).unwrap();
                    let mut worker = rv1.enter(id, &p1).unwrap();
                    let mut next = rv2.enter(id, &p2).unwrap();
                    let mut db3 = rv1.enter(id, &p3).unwrap();
                    let mut db4 = rv1.enter(id, &p4).unwrap();
                    tokio::time::timeout(core::time::Duration::from_secs(1), async {
                        tokio::try_join!(
                            biased;
                            async {
                                match choice {
                                    0 => {
                                        sender.send::<LeftKnow>(&()).await?;
                                        sender.recv::<LeftReply>().await?;
                                    }
                                    1 => {
                                        sender.send::<InnerLeftKnow>(&()).await?;
                                        sender.recv::<InnerLeftReply>().await?;
                                    }
                                    2 => sender.send::<InnerRight>(&()).await?,
                                    _ => unreachable!(),
                                }
                                Ok(())
                            },
                            async {
                                let label = if worker_first {
                                    tokio::select! {
                                        biased;
                                        branch = worker.offer() => branch?.label(),
                                        branch = db3.offer() => branch?.label(),
                                        branch = db4.offer() => branch?.label(),
                                    }
                                } else {
                                    tokio::select! {
                                        biased;
                                        branch = db4.offer() => branch?.label(),
                                        branch = db3.offer() => branch?.label(),
                                        branch = worker.offer() => branch?.label(),
                                    }
                                };
                                assert_eq!(label, choice * 2, "choice={choice} worker_first={worker_first}");
                                match label {
                                    0 => {
                                        db3.offer().await?.recv::<LeftKnow>().await?;
                                        let branch = worker.offer().await?;
                                        assert_eq!(branch.label(), 1);
                                        branch.send::<LeftReply>(&()).await?;
                                    }
                                    2 => {
                                        db4.offer().await?.recv::<InnerLeftKnow>().await?;
                                        let branch = worker.offer().await?;
                                        assert_eq!(branch.label(), 3);
                                        branch.send::<InnerLeftReply>(&()).await?;
                                    }
                                    4 => {
                                        worker.offer().await?.recv::<InnerRight>().await?;
                                    }
                                    _ => panic!("unexpected choice"),
                                }
                                worker.send::<Loan3<'_>>(db3).await?;
                                worker.send::<Loan4<'_>>(db4).await
                            },
                            async {
                                let _db3 = next.recv::<Loan3<'_>>().await?;
                                let _db4 = next.recv::<Loan4<'_>>().await?;
                                Ok(())
                            },
                        )
                    })
                    .await
                    .unwrap_or_else(|_| {
                        panic!("nested choice did not progress: choice={choice} worker_first={worker_first}")
                    })
                    .unwrap_or_else(|error| {
                        panic!("choice={choice}, worker_first={worker_first}: {error:?}")
                    });
                    assert!(network.queue_is_empty());
                }
            }
        });
}
