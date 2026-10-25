//! Parallel borrows retain their repeat origin across nested scopes and pending I/O.
mod common;
use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

type Prefix = g::Msg<231, ()>;
type Work = g::Msg<232, ()>;
type Ack = g::Msg<233, ()>;
type Tick = g::Msg<234, ()>;

#[test]
fn a_resolved_choice_preview_is_shared_by_the_holders_parallel_views() {
    use hibana::{
        Endpoint, EndpointError,
        runtime::resolver::{DecisionArm, ResolverRef},
    };
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type LeftData = g::Msg<1, ()>;
    type LeftNotice = g::Msg<2, ()>;
    type RightData = g::Msg<3, ()>;
    type RightNotice = g::Msg<4, ()>;
    type Returned<'r> = g::Msg<5, Endpoint<'r, 3>>;

    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::route(
                g::par(g::send::<0, 3, LeftData>(), g::send::<0, 1, LeftNotice>()),
                g::par(g::send::<0, 3, RightData>(), g::send::<0, 1, RightNotice>()),
            )
            .resolve::<0>(),
            g::send::<1, 0, Returned<'_>>(),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p3 = project::<3>(&global);

    for arm in [DecisionArm::Left, DecisionArm::Right] {
        let network = common::TestTransport::new();
        let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        rv0.set_resolver(&p0, ResolverRef::<0>::decision_state(&arm, |arm| Ok(*arm)))
            .unwrap();
        let sid = SessionId::new(9400);
        let mut owner = rv0.enter(sid, &p0).unwrap();
        let db = rv0.enter(sid, &p3).unwrap();
        let mut worker = rv1.enter(sid, &p1).unwrap();

        let owner_run = async {
            owner.send::<Loan<'_>>(db).await?;
            let label = owner.offer().await?.label();
            {
                let [mut data, mut notice] = owner.par()?;
                match label {
                    1 | 2 => {
                        tokio::try_join!(
                            data.send::<LeftData>(&()),
                            notice.send::<LeftNotice>(&())
                        )?;
                    }
                    3 | 4 => {
                        tokio::try_join!(
                            data.send::<RightData>(&()),
                            notice.send::<RightNotice>(&())
                        )?;
                    }
                    _ => panic!("unexpected controller label"),
                }
            }
            let _returned = owner.recv::<Returned<'_>>().await?;
            Ok::<_, EndpointError>(())
        };
        let worker_run = async {
            let mut db = worker.recv::<Loan<'_>>().await?;
            let label = tokio::select! {
                result = async {
                    let branch = worker.offer().await?;
                    let label = branch.label();
                    drop(branch);
                    Ok::<_, EndpointError>(label)
                } => result?,
                result = async {
                    let branch = db.offer().await?;
                    let label = branch.label();
                    drop(branch);
                    Ok::<_, EndpointError>(label)
                } => result?,
            };
            {
                let [mut notice] = worker.par()?;
                let [mut data] = db.par()?;
                match label {
                    1 | 2 => {
                        tokio::try_join!(data.recv::<LeftData>(), notice.recv::<LeftNotice>())?;
                    }
                    3 | 4 => {
                        tokio::try_join!(data.recv::<RightData>(), notice.recv::<RightNotice>())?;
                    }
                    _ => panic!("unexpected received label"),
                }
            }
            worker.send::<Returned<'_>>(db).await?;
            Ok::<_, EndpointError>(())
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(core::time::Duration::from_secs(1), async {
                    tokio::try_join!(owner_run, worker_run)
                })
                .await
                .expect("parallel views stalled")
                .unwrap();
            });
        assert!(network.queue_is_empty());
    }
}

#[test]
fn a_borrow_containing_a_choice_uses_its_other_views_preview() {
    use hibana::{
        Endpoint, EndpointError,
        runtime::resolver::{DecisionArm, ResolverRef},
    };
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type LeftData = g::Msg<1, ()>;
    type LeftNotice = g::Msg<2, ()>;
    type RightData = g::Msg<3, ()>;
    type RightNotice = g::Msg<4, ()>;
    type Returned<'r> = g::Msg<5, Endpoint<'r, 3>>;
    type Tick = g::Msg<6, ()>;
    type Ack = g::Msg<7, ()>;

    let global = g::par(
        g::seq(
            g::send::<0, 1, Loan<'_>>(),
            g::seq(
                g::route(
                    g::par(g::send::<0, 3, LeftData>(), g::send::<0, 1, LeftNotice>()),
                    g::par(g::send::<0, 3, RightData>(), g::send::<0, 1, RightNotice>()),
                )
                .resolve::<0>(),
                g::send::<1, 0, Returned<'_>>(),
            ),
        ),
        g::seq(g::send::<2, 0, Tick>(), g::send::<0, 2, Ack>()),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    for arm in [DecisionArm::Left, DecisionArm::Right] {
        let network = common::TestTransport::new();
        let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
        let mut s0 = SessionKitStorage::uninit();
        let mut s1 = SessionKitStorage::uninit();
        let mut s2 = SessionKitStorage::uninit();
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
        rv0.set_resolver(&p0, ResolverRef::<0>::decision_state(&arm, |arm| Ok(*arm)))
            .unwrap();
        let sid = SessionId::new(9401);
        let mut owner = rv0.enter(sid, &p0).unwrap();
        let db = rv0.enter(sid, &p3).unwrap();
        let mut worker = rv1.enter(sid, &p1).unwrap();
        let mut peer = rv2.enter(sid, &p2).unwrap();

        let owner_run = async {
            let [mut work, mut ticks] = owner.par().expect("owner's outer parallel parts");
            tokio::try_join!(
                async {
                    work.send::<Loan<'_>>(db).await?;
                    let label = work.offer().await?.label();
                    {
                        let [mut data, mut notice] =
                            work.par().expect("controller's selected parallel parts");
                        match label {
                            1 | 2 => {
                                tokio::try_join!(
                                    data.send::<LeftData>(&()),
                                    notice.send::<LeftNotice>(&())
                                )?;
                            }
                            3 | 4 => {
                                tokio::try_join!(
                                    data.send::<RightData>(&()),
                                    notice.send::<RightNotice>(&())
                                )?;
                            }
                            _ => panic!("unexpected controller label"),
                        }
                    }
                    let _returned = work.recv::<Returned<'_>>().await?;
                    Ok::<_, EndpointError>(())
                },
                async {
                    ticks.recv::<Tick>().await?;
                    ticks.send::<Ack>(&()).await?;
                    Ok::<_, EndpointError>(())
                }
            )?;
            Ok::<_, EndpointError>(())
        };
        let worker_run = async {
            let [mut work] = worker.par().expect("worker's outer parallel part");
            let mut db = work.recv::<Loan<'_>>().await?;
            {
                let [mut db_work] = db.par().expect("resource's outer parallel part");
                let label = tokio::select! {
                    result = async {
                        let branch = work.offer().await?;
                        let label = branch.label();
                        drop(branch);
                        Ok::<_, EndpointError>(label)
                    } => result?,
                    result = async {
                        let branch = db_work.offer().await?;
                        let label = branch.label();
                        drop(branch);
                        Ok::<_, EndpointError>(label)
                    } => result?,
                };
                {
                    let [mut notice] = work.par().expect("worker's selected parallel part");
                    let [mut data] = db_work.par().expect("resource's selected parallel part");
                    match label {
                        1 | 2 => {
                            tokio::try_join!(data.recv::<LeftData>(), notice.recv::<LeftNotice>())?;
                        }
                        3 | 4 => {
                            tokio::try_join!(
                                data.recv::<RightData>(),
                                notice.recv::<RightNotice>()
                            )?;
                        }
                        _ => panic!("unexpected received label"),
                    }
                }
            }
            work.send::<Returned<'_>>(db).await?;
            Ok::<_, EndpointError>(())
        };
        let peer_run = async {
            let [mut ticks] = peer.par().expect("peer's independent parallel part");
            ticks.send::<Tick>(&()).await?;
            ticks.recv::<Ack>().await?;
            Ok::<_, EndpointError>(())
        };
        let results = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(core::time::Duration::from_secs(1), async {
                    tokio::join!(owner_run, worker_run, peer_run)
                })
                .await
                .expect("borrowed parallel views stalled")
            });
        assert!(matches!(&results, (Ok(()), Ok(()), Ok(()))), "{results:?}");
        assert!(network.queue_is_empty());
    }
}

#[test]
fn a_delegated_parallel_prefix_repeats_with_its_sequential_tail() {
    use hibana::Endpoint;
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type LeftRead = g::Msg<1, ()>;
    type LeftNotice = g::Msg<2, ()>;
    type LeftValue = g::Msg<3, ()>;
    type RightRead = g::Msg<4, ()>;
    type RightValue = g::Msg<5, ()>;
    type TailRead = g::Msg<6, ()>;
    type TailNotice = g::Msg<7, ()>;
    type TailValue = g::Msg<8, ()>;
    type Return<'r> = g::Msg<9, Endpoint<'r, 3>>;
    let global = g::seq(
        g::send::<1, 0, Loan<'_>>(),
        g::seq(
            g::seq(
                g::par(
                    g::seq(
                        g::send::<3, 2, LeftRead>(),
                        g::seq(g::send::<0, 2, LeftNotice>(), g::send::<2, 3, LeftValue>()),
                    ),
                    g::seq(g::send::<3, 2, RightRead>(), g::send::<2, 3, RightValue>()),
                ),
                g::seq(
                    g::send::<3, 2, TailRead>(),
                    g::seq(g::send::<0, 2, TailNotice>(), g::send::<2, 3, TailValue>()),
                ),
            )
            .roll(),
            g::send::<0, 1, Return<'_>>(),
        ),
    );
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9389);
    let mut holder = rv0.enter(sid, &p0).unwrap();
    let mut owner = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let db = rv1.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::try_join!(
                    async {
                        let mut db = holder.recv::<Loan<'_>>().await?;
                        for visit in 0..2 {
                            {
                                let [mut notice] = holder.par().unwrap_or_else(|error| {
                                    panic!("holder par visit {visit}: {error:?}")
                                });
                                let [mut left, mut right] = db.par().unwrap_or_else(|error| {
                                    panic!("resource par visit {visit}: {error:?}")
                                });
                                tokio::try_join!(
                                    async {
                                        left.send::<LeftRead>(&()).await?;
                                        notice.send::<LeftNotice>(&()).await?;
                                        left.recv::<LeftValue>().await?;
                                        Ok::<_, hibana::EndpointError>(())
                                    },
                                    async {
                                        right.send::<RightRead>(&()).await?;
                                        right.recv::<RightValue>().await?;
                                        Ok::<_, hibana::EndpointError>(())
                                    },
                                )?;
                            }
                            db.send::<TailRead>(&()).await?;
                            holder.send::<TailNotice>(&()).await?;
                            db.recv::<TailValue>().await?;
                        }
                        holder.send::<Return<'_>>(db).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        owner.send::<Loan<'_>>(db).await?;
                        let db = owner.recv::<Return<'_>>().await?;
                        drop(db);
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        for visit in 0..2 {
                            {
                                let [mut left, mut right] = service.par().unwrap_or_else(|error| {
                                    panic!("service par visit {visit}: {error:?}")
                                });
                                tokio::try_join!(
                                    async {
                                        left.recv::<LeftRead>().await?;
                                        left.recv::<LeftNotice>().await?;
                                        left.send::<LeftValue>(&()).await?;
                                        Ok::<_, hibana::EndpointError>(())
                                    },
                                    async {
                                        right.recv::<RightRead>().await?;
                                        right.send::<RightValue>(&()).await?;
                                        Ok::<_, hibana::EndpointError>(())
                                    },
                                )?;
                            }
                            service.recv::<TailRead>().await?;
                            service.recv::<TailNotice>().await?;
                            service.send::<TailValue>(&()).await?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    },
                )
            })
            .await
            .expect("delegated parallel prefix stalled")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

fn returned_resource_parallel_global() -> impl hibana::runtime::program::Projectable {
    use hibana::Endpoint;
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type VisitLoan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type LeftRead = g::Msg<2, u32>;
    type LeftNotice = g::Msg<3, u32>;
    type LeftValue = g::Msg<4, u32>;
    type RightRead = g::Msg<5, u32>;
    type RightNotice = g::Msg<6, u32>;
    type RightValue = g::Msg<7, u32>;
    type VisitReturn<'r> = g::Msg<8, Endpoint<'r, 3>>;
    type FinalReturn<'r> = g::Msg<9, Endpoint<'r, 3>>;
    g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::seq(
                g::send::<1, 0, VisitLoan<'_>>(),
                g::seq(
                    g::par(
                        g::seq(
                            g::send::<3, 2, LeftRead>(),
                            g::seq(g::send::<0, 2, LeftNotice>(), g::send::<2, 3, LeftValue>()),
                        ),
                        g::seq(
                            g::send::<3, 2, RightRead>(),
                            g::seq(
                                g::send::<0, 2, RightNotice>(),
                                g::send::<2, 3, RightValue>(),
                            ),
                        ),
                    ),
                    g::send::<0, 1, VisitReturn<'_>>(),
                ),
            )
            .roll(),
            g::send::<1, 0, FinalReturn<'_>>(),
        ),
    )
}

#[test]
fn a_returned_resource_and_its_holder_borrow_both_parallel_arms_on_each_visit() {
    use hibana::Endpoint;
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type VisitLoan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type LeftRead = g::Msg<2, u32>;
    type LeftNotice = g::Msg<3, u32>;
    type LeftValue = g::Msg<4, u32>;
    type RightRead = g::Msg<5, u32>;
    type RightNotice = g::Msg<6, u32>;
    type RightValue = g::Msg<7, u32>;
    type VisitReturn<'r> = g::Msg<8, Endpoint<'r, 3>>;
    type FinalReturn<'r> = g::Msg<9, Endpoint<'r, 3>>;
    let global = returned_resource_parallel_global();
    assert_eq!(g::diagnose(&global), None);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9393);
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
                tokio::try_join!(
                    async {
                        assert!(owner.par::<2>().is_err(), "the initial loan is required");
                        owner.send::<Loan<'_>>(db).await?;
                        for visit in 0..2 {
                            let mut db = owner.recv::<VisitLoan<'_>>().await?;
                            {
                                let [mut left_notice, mut right_notice] =
                                    owner.par().unwrap_or_else(|error| {
                                        panic!("holder par visit={visit}: {error:?}")
                                    });
                                let [mut left_db, mut right_db] =
                                    db.par().unwrap_or_else(|error| {
                                        panic!("resource par visit={visit}: {error:?}")
                                    });
                                let (left, right) = tokio::try_join!(
                                    async {
                                        left_db.send::<LeftRead>(&visit).await?;
                                        left_notice.send::<LeftNotice>(&10).await?;
                                        left_db.recv::<LeftValue>().await
                                    },
                                    async {
                                        right_db.send::<RightRead>(&visit).await?;
                                        right_notice.send::<RightNotice>(&20).await?;
                                        right_db.recv::<RightValue>().await
                                    },
                                )?;
                                assert_eq!((left, right), (visit + 10, visit + 20));
                            }
                            owner.send::<VisitReturn<'_>>(db).await?;
                        }
                        drop(owner.recv::<FinalReturn<'_>>().await?);
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        let mut db = worker.recv::<Loan<'_>>().await?;
                        for _ in 0..2 {
                            worker.send::<VisitLoan<'_>>(db).await?;
                            db = worker.recv::<VisitReturn<'_>>().await?;
                        }
                        worker.send::<FinalReturn<'_>>(db).await
                    },
                    async {
                        for visit in 0..2 {
                            let [mut left, mut right] = service.par().unwrap_or_else(|error| {
                                panic!("service par visit={visit}: {error:?}")
                            });
                            tokio::try_join!(
                                async {
                                    let read = left.recv::<LeftRead>().await?;
                                    let notice = left.recv::<LeftNotice>().await?;
                                    left.send::<LeftValue>(&(read + notice)).await
                                },
                                async {
                                    let read = right.recv::<RightRead>().await?;
                                    let notice = right.recv::<RightNotice>().await?;
                                    right.send::<RightValue>(&(read + notice)).await
                                },
                            )?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    },
                )
            })
            .await
            .expect("parallel work in a returned resource stopped")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_retained_completion_cannot_send_before_the_next_visits_own_loan() {
    use core::{future::Future, task::Poll};
    use hibana::Endpoint;
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type VisitLoan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type LeftRead = g::Msg<2, u32>;
    type LeftNotice = g::Msg<3, u32>;
    type LeftValue = g::Msg<4, u32>;
    type RightRead = g::Msg<5, u32>;
    type RightNotice = g::Msg<6, u32>;
    type RightValue = g::Msg<7, u32>;
    type VisitReturn<'r> = g::Msg<8, Endpoint<'r, 3>>;
    let global = returned_resource_parallel_global();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9394);
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
                owner.send::<Loan<'_>>(db).await.unwrap();
                let mut db = worker.recv::<Loan<'_>>().await.unwrap();
                for visit in 0..2 {
                    worker.send::<VisitLoan<'_>>(db).await.unwrap();
                    db = owner.recv::<VisitLoan<'_>>().await.unwrap();
                    {
                        let [mut left_notice, mut right_notice] = owner.par().unwrap();
                        let [mut left_db, mut right_db] = db.par().unwrap();
                        let [mut left_service, mut right_service] = service.par().unwrap();
                        tokio::try_join!(
                            async {
                                left_db.send::<LeftRead>(&visit).await?;
                                tokio::task::yield_now().await;
                                let value = left_service.recv::<LeftRead>().await?;
                                left_notice.send::<LeftNotice>(&10).await?;
                                let notice = left_service.recv::<LeftNotice>().await?;
                                left_service.send::<LeftValue>(&(value + notice)).await?;
                                assert_eq!(left_db.recv::<LeftValue>().await?, visit + 10);
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                right_db.send::<RightRead>(&visit).await?;
                                tokio::task::yield_now().await;
                                let value = right_service.recv::<RightRead>().await?;
                                right_notice.send::<RightNotice>(&20).await?;
                                let notice = right_service.recv::<RightNotice>().await?;
                                right_service.send::<RightValue>(&(value + notice)).await?;
                                assert_eq!(right_db.recv::<RightValue>().await?, visit + 20);
                                Ok::<_, hibana::EndpointError>(())
                            },
                        )
                        .unwrap();
                    }
                    owner.send::<VisitReturn<'_>>(db).await.unwrap();
                    db = worker.recv::<VisitReturn<'_>>().await.unwrap();
                }
                assert!(network.queue_is_empty());
                let future = owner.send::<LeftNotice>(&99);
                tokio::pin!(future);
                let result =
                    core::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await;
                assert!(
                    matches!(result, Poll::Ready(Err(_))),
                    "next visit skipped its loan: {result:?}"
                );
                assert!(
                    network.queue_is_empty(),
                    "an out-of-order notice reached the service"
                );
            })
            .await
            .expect("preparing the retained completion stalled");
        });
}

#[test]
fn a_parallel_repeat_can_continue_before_an_enclosing_choice_restarts() {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type First = g::Msg<140, ()>;
    type FirstReply = g::Msg<141, ()>;
    type Second = g::Msg<142, ()>;
    type SecondReply = g::Msg<143, ()>;
    type Other = g::Msg<144, ()>;
    type OtherReply = g::Msg<145, ()>;
    let global = g::route(
        g::par(
            g::seq(g::send::<0, 1, First>(), g::send::<1, 0, FirstReply>()),
            g::seq(g::send::<0, 1, Second>(), g::send::<1, 0, SecondReply>()),
        )
        .roll(),
        g::seq(g::send::<0, 1, Other>(), g::send::<1, 0, OtherReply>()),
    )
    .resolve::<0>()
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let sid = SessionId::new(9392);
    let mut sender = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::try_join!(
                    biased;
                    async {
                        assert_eq!(sender.offer().await?.label(), 140);
                        for _ in 0..2 {
                            let [mut first, mut second] = sender.par()?;
                            tokio::try_join!(
                                async {
                                    first.send::<First>(&()).await?;
                                    first.recv::<FirstReply>().await
                                },
                                async {
                                    second.send::<Second>(&()).await?;
                                    second.recv::<SecondReply>().await
                                },
                            )?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        assert_eq!(receiver.offer().await?.label(), 140);
                        for visit in 0..2 {
                            let [mut first, mut second] = receiver.par().unwrap_or_else(|error| {
                                panic!("receiver par visit={visit}: {error:?}")
                            });
                            tokio::try_join!(
                                async {
                                    first.recv::<First>().await?;
                                    first.send::<FirstReply>(&()).await
                                },
                                async {
                                    second.recv::<Second>().await?;
                                    second.send::<SecondReply>(&()).await
                                },
                            )?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    },
                )
            })
            .await
            .expect("parallel repeat must finish")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_rolled_parallel_borrow_can_repeat_before_its_sequential_tail() {
    type RequestA = g::Msg<120, ()>;
    type ReplyA = g::Msg<121, ()>;
    type RequestB = g::Msg<122, ()>;
    type ReplyB = g::Msg<123, ()>;
    type Tail = g::Msg<124, ()>;
    type TailReply = g::Msg<125, ()>;
    type Outside = g::Msg<126, ()>;
    type OutsideReply = g::Msg<127, ()>;
    let global = g::par(
        g::seq(
            g::par(
                g::seq(g::send::<0, 1, RequestA>(), g::send::<1, 0, ReplyA>()),
                g::seq(g::send::<0, 1, RequestB>(), g::send::<1, 0, ReplyB>()).roll(),
            )
            .roll(),
            g::seq(g::send::<0, 1, Tail>(), g::send::<1, 0, TailReply>()),
        ),
        g::seq(g::send::<0, 1, Outside>(), g::send::<1, 0, OutsideReply>()),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let rv0 = storage0
        .init()
        .rendezvous(&mut slab0, network.clone())
        .unwrap();
    let rv1 = storage1
        .init()
        .rendezvous(&mut slab1, network.clone())
        .unwrap();
    let sid = SessionId::new(9391);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                let [mut flow_a, mut outside_a] = a.par()?;
                let [mut flow_b, mut outside_b] = b.par()?;
                tokio::try_join!(
                    async {
                        for visit in 0..2 {
                            let [mut first, mut repeated] = flow_a.par().unwrap_or_else(|error| {
                                panic!("sender par visit={visit}: {error:?}")
                            });
                            tokio::try_join!(
                                async {
                                    first.send::<RequestA>(&()).await?;
                                    first.recv::<ReplyA>().await?;
                                    Ok::<_, hibana::EndpointError>(())
                                },
                                async {
                                    for _ in 0..2 {
                                        repeated.send::<RequestB>(&()).await?;
                                        repeated.recv::<ReplyB>().await?;
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                },
                            )?;
                        }
                        flow_a.send::<Tail>(&()).await?;
                        flow_a.recv::<TailReply>().await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        for visit in 0..2 {
                            let [mut first, mut repeated] = flow_b.par().unwrap_or_else(|error| {
                                panic!("receiver par visit={visit}: {error:?}")
                            });
                            tokio::try_join!(
                                async {
                                    first.recv::<RequestA>().await?;
                                    first.send::<ReplyA>(&()).await?;
                                    Ok::<_, hibana::EndpointError>(())
                                },
                                async {
                                    for _ in 0..2 {
                                        repeated.recv::<RequestB>().await?;
                                        repeated.send::<ReplyB>(&()).await?;
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                },
                            )?;
                        }
                        flow_b.recv::<Tail>().await?;
                        flow_b.send::<TailReply>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        outside_a.send::<Outside>(&()).await?;
                        outside_a.recv::<OutsideReply>().await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        outside_b.recv::<Outside>().await?;
                        outside_b.send::<OutsideReply>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                )?;
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("nested repeated borrows must finish")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_parallel_borrow_rejects_its_own_cross_lane_prefix() {
    type Note = g::Msg<246, ()>;
    let global = g::par(
        g::seq(
            g::send::<0, 1, Prefix>(),
            g::par(
                g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()),
                g::send::<0, 2, Tick>(),
            )
            .roll(),
        ),
        g::send::<0, 3, Note>(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2, mut slab3] = [[0; 32 * 1024]; 4];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let mut storage3 = SessionKitStorage::uninit();
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
    let rv3 = storage3
        .init()
        .rendezvous(&mut slab3, network.clone())
        .unwrap();
    let sid = SessionId::new(9390);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    let mut d = rv3.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for _ in 0..2 {
                let [mut flow, mut note] = a.par().unwrap();
                tokio::try_join!(
                    async {
                        flow.send::<Prefix>(&()).await?;
                        let [mut work, mut tick] = flow.par()?;
                        tokio::try_join!(
                            async {
                                work.send::<Work>(&()).await?;
                                work.recv::<Ack>().await?;
                                Ok::<_, hibana::EndpointError>(())
                            },
                            tick.send::<Tick>(&()),
                        )?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    note.send::<Note>(&()),
                    async {
                        b.recv::<Prefix>().await?;
                        b.recv::<Work>().await?;
                        b.send::<Ack>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    c.recv::<Tick>(),
                    d.recv::<Note>(),
                )
                .unwrap();
            }
            assert!(network.queue_is_empty());
            let [mut flow, _note] = a.par().unwrap();
            let result =
                tokio::time::timeout(core::time::Duration::from_millis(1), flow.send::<Tick>(&()))
                    .await
                    .expect("the borrow must reject its own prefix instead of waiting for it");
            assert!(result.is_err());
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_child_repeat_keeps_its_prefix_on_another_lane() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type TailA = g::Msg<244, ()>;
            type TailB = g::Msg<245, ()>;
            let global = g::seq(
                g::send::<0, 1, Prefix>(),
                g::seq(
                    g::par(g::send::<0, 1, Work>(), g::send::<0, 2, Tick>()).roll(),
                    g::par(g::send::<0, 1, TailA>(), g::send::<0, 2, TailB>()),
                ),
            )
            .roll();
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
            let sid = SessionId::new(9303);
            let mut a = rv0.enter(sid, &p0).unwrap();
            let mut b = rv1.enter(sid, &p1).unwrap();
            let mut c = rv2.enter(sid, &p2).unwrap();
            a.send::<Prefix>(&()).await.unwrap();
            b.recv::<Prefix>().await.unwrap();
            {
                let [mut work, mut tick] = a.par().unwrap();
                tokio::try_join!(
                    work.send::<Work>(&()),
                    tick.send::<Tick>(&()),
                    b.recv::<Work>(),
                    c.recv::<Tick>()
                )
                .unwrap();
            }
            {
                let [mut tail_a, mut tail_b] = a.par().unwrap();
                tokio::try_join!(
                    tail_a.send::<TailA>(&()),
                    tail_b.send::<TailB>(&()),
                    b.recv::<TailA>(),
                    c.recv::<TailB>()
                )
                .unwrap();
            }
            // Probe the second lane first, after both continuation lanes completed.
            a.send::<Tick>(&())
                .await
                .expect("the child retains its prefix on another lane");
            c.recv::<Tick>().await.unwrap();
            a.send::<Work>(&()).await.unwrap();
            b.recv::<Work>().await.unwrap();
            a.send::<Prefix>(&()).await.unwrap();
            b.recv::<Prefix>().await.unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn an_inner_repeat_after_a_same_lane_tail_keeps_its_outer_prefix() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = g::seq(
                g::send::<0, 1, Prefix>(),
                g::seq(
                    g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
                    g::send::<0, 1, Tick>(),
                ),
            )
            .roll();
            assert_eq!(g::diagnose(&global), None);
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let network = common::TestTransport::new();
            let [mut slab0, mut slab1] = [[0; 32 * 1024]; 2];
            let mut storage0 = SessionKitStorage::uninit();
            let mut storage1 = SessionKitStorage::uninit();
            let rv0 = storage0
                .init()
                .rendezvous(&mut slab0, network.clone())
                .unwrap();
            let rv1 = storage1
                .init()
                .rendezvous(&mut slab1, network.clone())
                .unwrap();
            let sid = SessionId::new(9301);
            let mut a = rv0.enter(sid, &p0).unwrap();
            let mut b = rv1.enter(sid, &p1).unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    a.send::<Prefix>(&()).await?;
                    b.recv::<Prefix>().await?;
                    a.send::<Work>(&()).await?;
                    b.recv::<Work>().await?;
                    b.send::<Ack>(&()).await?;
                    a.recv::<Ack>().await?;
                    a.send::<Tick>(&()).await?;
                    b.recv::<Tick>().await?;
                    // Continue the child within this same enclosing visit.
                    a.send::<Work>(&())
                        .await
                        .expect("continue the old inner send after the tail");
                    b.recv::<Work>()
                        .await
                        .expect("receive the continued inner work");
                    b.send::<Ack>(&())
                        .await
                        .expect("reply in the continued inner visit");
                    a.recv::<Ack>()
                        .await
                        .expect("receive the continued inner reply");
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("inner continuation must not wait for a fresh outer prefix")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_delegated_child_after_a_same_lane_tail_keeps_its_outer_prefix() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            use hibana::Endpoint;
            type Loan<'a> = g::Msg<238, Endpoint<'a, 3>>;
            type Read = g::Msg<239, u32>;
            type Value = g::Msg<240, u32>;
            let global = g::seq(
                g::send::<0, 1, Prefix>(),
                g::seq(
                    g::seq(
                        g::send::<0, 1, Loan<'_>>(),
                        g::seq(
                            g::send::<3, 2, Read>(),
                            g::seq(g::send::<2, 3, Value>(), g::send::<1, 0, Loan<'_>>()),
                        ),
                    )
                    .roll(),
                    g::send::<0, 1, Tick>(),
                ),
            )
            .roll();
            assert_eq!(g::diagnose(&global), None);
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let p3 = project::<3>(&global);
            let network = common::TestTransport::new();
            let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
            let sid = SessionId::new(9302);
            let mut owner = rv0.enter(sid, &p0).unwrap();
            let mut worker = rv1.enter(sid, &p1).unwrap();
            let mut service = rv2.enter(sid, &p2).unwrap();
            let mut db = rv0.enter(sid, &p3).unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for visit in 0..2 {
                    owner.send::<Prefix>(&()).await?;
                    worker.recv::<Prefix>().await?;
                    for child in 0..2 {
                        owner
                            .send::<Loan<'_>>(db)
                            .await
                            .expect("loan within the current outer visit");
                        db = worker.recv::<Loan<'_>>().await?;
                        db.send::<Read>(&(10 * visit + child)).await?;
                        let number = service.recv::<Read>().await?;
                        service.send::<Value>(&(number + 1)).await?;
                        assert_eq!(db.recv::<Value>().await?, number + 1);
                        worker.send::<Loan<'_>>(db).await?;
                        db = owner.recv::<Loan<'_>>().await?;
                        if child == 0 {
                            owner.send::<Tick>(&()).await?;
                            worker.recv::<Tick>().await?;
                        }
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("delegated child continuation must keep its enclosing prefix")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_single_fragment_still_requires_a_parallel_source() {
    let global = g::seq(g::send::<0, 1, Work>(), g::send::<0, 1, Tick>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let network = common::TestTransport::new();
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(9300);
    let mut a = rv.enter(sid, &p0).unwrap();
    let mut b = rv.enter(sid, &p1).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            assert!(a.par::<1>().is_err());
            a.send::<Work>(&()).await.unwrap();
            b.recv::<Work>().await.unwrap();
            a.send::<Tick>(&()).await.unwrap();
            b.recv::<Tick>().await.unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn separate_endpoint_views_enter_the_same_parallel_repeat() {
    use hibana::Endpoint;
    type Loan = g::Msg<235, Endpoint<'static, 3>>;
    type Read = g::Msg<236, u32>;
    type Value = g::Msg<237, u32>;
    let global = g::seq(
        g::seq(g::send::<0, 1, Loan>(), g::send::<1, 0, Loan>()),
        g::par(
            g::seq(g::send::<3, 2, Read>(), g::send::<2, 3, Value>()).roll(),
            g::send::<0, 1, Tick>(),
        )
        .roll(),
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
    let sid = SessionId::new(9299);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let mut db = rv0.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            owner.send::<Loan>(db).await.unwrap();
            db = worker.recv::<Loan>().await.unwrap();
            worker.send::<Loan>(db).await.unwrap();
            db = owner.recv::<Loan>().await.unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for visit in 0..3 {
                    let [mut reads] = db.par()?;
                    let [mut sent_tick] = owner.par()?;
                    let [mut jobs] = service.par()?;
                    let [mut received_tick] = worker.par()?;
                    if visit == 1 {
                        network.pause_send(3);
                    }
                    tokio::try_join!(
                        biased;
                        async {
                            for value in [7, 11] {
                                reads.send::<Read>(&value).await?;
                                assert_eq!(reads.recv::<Value>().await?, value + 1);
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                        async {
                            sent_tick.send::<Tick>(&()).await?;
                            if visit == 1 {
                                assert!(network.paused_send_is_waiting(3));
                                network.resume_send();
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                        async {
                            for _ in 0..2 {
                                let value = jobs.recv::<Read>().await?;
                                jobs.send::<Value>(&(value + 1)).await?;
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                        received_tick.recv::<Tick>(),
                    )?;
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("independent endpoint views must not serialize")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

fn global() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(
            g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
            g::send::<0, 2, Tick>(),
        )
        .roll(),
    )
    .roll()
}

#[test]
fn a_choice_wrapped_parallel_repeat_keeps_its_repeat_origin() {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type OtherWork = g::Msg<241, ()>;
    type OtherAck = g::Msg<242, ()>;
    type OtherTick = g::Msg<243, ()>;
    let global = g::route(
        g::par(
            g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
            g::send::<0, 2, Tick>(),
        ),
        g::par(
            g::seq(g::send::<0, 1, OtherWork>(), g::send::<1, 0, OtherAck>()).roll(),
            g::send::<0, 2, OtherTick>(),
        ),
    )
    .resolve::<7>()
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let input = (
        core::cell::Cell::new(DecisionArm::Left),
        core::cell::Cell::new(0usize),
    );
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<7>::decision_state(&input, |(choice, calls)| {
            calls.set(calls.get() + 1);
            Ok(choice.get())
        }),
    )
    .unwrap();
    let sid = SessionId::new(9297);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for choice in [
                    DecisionArm::Left,
                    DecisionArm::Left,
                    DecisionArm::Right,
                    DecisionArm::Right,
                    DecisionArm::Left,
                ] {
                    input.0.set(choice);
                    tokio::try_join!(
                        biased;
                        async {
                            let label = a.offer().await?.label();
                            match label {
                                232 => {
                                    let [mut exchanges, mut tick] = a.par()?;
                                    tokio::try_join!(
                                        async {
                                            for _ in 0..2 {
                                                exchanges.send::<Work>(&()).await?;
                                                exchanges.recv::<Ack>().await?;
                                            }
                                            Ok::<_, hibana::EndpointError>(())
                                        },
                                        tick.send::<Tick>(&()),
                                    )?;
                                }
                                241 => {
                                    let [mut exchanges, mut tick] = a.par()?;
                                    tokio::try_join!(
                                        async {
                                            for _ in 0..2 {
                                                exchanges.send::<OtherWork>(&()).await?;
                                                exchanges.recv::<OtherAck>().await?;
                                            }
                                            Ok::<_, hibana::EndpointError>(())
                                        },
                                        tick.send::<OtherTick>(&()),
                                    )?;
                                }
                                label => panic!("unexpected controller branch {label}"),
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                        async {
                            let label = b.offer().await?.label();
                            match label {
                                232 => {
                                    for _ in 0..2 {
                                        b.recv::<Work>().await?;
                                        b.send::<Ack>(&()).await?;
                                    }
                                }
                                241 => {
                                    for _ in 0..2 {
                                        b.recv::<OtherWork>().await?;
                                        b.send::<OtherAck>(&()).await?;
                                    }
                                }
                                label => panic!("unexpected exchange branch {label}"),
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                        async {
                            let label = c.offer().await?.label();
                            match label {
                                234 => c.recv::<Tick>().await?,
                                243 => c.recv::<OtherTick>().await?,
                                label => panic!("unexpected notification branch {label}"),
                            }
                            Ok::<_, hibana::EndpointError>(())
                        },
                    )?;
                }
                assert_eq!(input.1.get(), 5, "one resolver call per outer visit");
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("choice-wrapped parallel repeats must finish")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn borrowing_an_old_parallel_continuation_keeps_its_outer_prefix() {
    let global = g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(
            g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
            g::send::<0, 2, Tick>(),
        ),
    )
    .roll();
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
    let sid = SessionId::new(9298);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    a.send::<Prefix>(&()).await?;
                    b.recv::<Prefix>().await?;
                    {
                        let [mut exchanges, mut tick] = a.par()?;
                        tokio::try_join!(
                            async {
                                exchanges.send::<Work>(&()).await?;
                                exchanges.recv::<Ack>().await?;
                                Ok::<_, hibana::EndpointError>(())
                            },
                            tick.send::<Tick>(&()),
                            async {
                                b.recv::<Work>().await?;
                                b.send::<Ack>(&()).await?;
                                Ok::<_, hibana::EndpointError>(())
                            },
                            c.recv::<Tick>(),
                        )?;
                    }
                    {
                        let [mut exchanges, _completed_tick] = a.par()?;
                        tokio::try_join!(
                            async {
                                exchanges.send::<Work>(&()).await?;
                                exchanges.recv::<Ack>().await?;
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                b.recv::<Work>().await?;
                                b.send::<Ack>(&()).await?;
                                Ok::<_, hibana::EndpointError>(())
                            },
                        )?;
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("parallel continuation must retain its prefix")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_new_parallel_visit_requires_its_own_sequential_prefix() {
    let global = g::par(
        g::seq(
            g::send::<0, 1, Prefix>(),
            g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
        ),
        g::send::<0, 2, Tick>(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9288);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for _ in 0..2 {
                let [mut exchanges, mut tick] = a.par().unwrap();
                tokio::try_join!(
                    async {
                        exchanges.send::<Prefix>(&()).await?;
                        exchanges.send::<Work>(&()).await?;
                        exchanges.recv::<Ack>().await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    tick.send::<Tick>(&()),
                    async {
                        b.recv::<Prefix>().await?;
                        b.recv::<Work>().await?;
                        b.send::<Ack>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    c.recv::<Tick>(),
                )
                .unwrap();
            }
            assert!(network.queue_is_empty());
            let [mut exchanges, _tick] = a.par().unwrap();
            assert!(exchanges.send::<Work>(&()).await.is_err());
            assert!(network.queue_is_empty());
        });
}

#[test]
fn nested_parallel_borrows_reenter_the_same_global() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = global();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let network = common::TestTransport::new();
            let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
            let sid = SessionId::new(9283);
            let mut a = rv0.enter(sid, &p0).unwrap();
            let mut b = rv1.enter(sid, &p1).unwrap();
            let mut c = rv2.enter(sid, &p2).unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    a.send::<Prefix>(&()).await?;
                    b.recv::<Prefix>().await?;
                    for _ in 0..2 {
                        let [mut exchanges, mut ticks] = a.par()?;
                        tokio::try_join!(
                            biased;
                            async {
                                for _ in 0..2 {
                                    exchanges.send::<Work>(&()).await?;
                                    exchanges.recv::<Ack>().await?;
                                }
                                Ok::<(),hibana::EndpointError>(())
                            },
                            ticks.send::<Tick>(&()),
                            async {
                                for _ in 0..2 {
                                    b.recv::<Work>().await?;
                                    b.send::<Ack>(&()).await?;
                                }
                                Ok::<(),hibana::EndpointError>(())
                            },
                            c.recv::<Tick>(),
                        )?;
                    }
                }
                Ok::<(), hibana::EndpointError>(())
            })
            .await
            .expect("nested projected fragments stalled")
            .unwrap();
            assert!(network.queue_is_empty());
        });
}

#[test]
fn nested_parallel_delegation_reenters_while_the_first_send_waits() {
    use hibana::Endpoint;
    type Loan = g::Msg<235, Endpoint<'static, 3>>;
    type Read = g::Msg<236, u32>;
    type Value = g::Msg<237, u32>;
    let global = g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(
            g::seq(
                g::send::<0, 1, Loan>(),
                g::seq(
                    g::send::<3, 2, Read>(),
                    g::seq(g::send::<2, 3, Value>(), g::send::<1, 0, Loan>()),
                ),
            )
            .roll(),
            g::send::<1, 0, Tick>(),
        )
        .roll(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9285);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let mut db = rv0.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    owner.send::<Prefix>(&()).await?;
                    worker.recv::<Prefix>().await?;
                    for visit in 0..2 {
                        let [mut loans, mut received_tick] = owner.par()?;
                        let [mut jobs, mut sent_tick] = worker.par()?;
                        if visit == 1 {
                            // I/O readiness only: the independent incoming Tick
                            // must complete while this outgoing loan is pending.
                            network.pause_send(0);
                        }
                        let (returned, (), (), (), ()) = tokio::try_join!(
                            biased;
                            async {
                                for _ in 0..2 {
                                    loans.send::<Loan>(db).await?;
                                    db = loans.recv::<Loan>().await?;
                                }
                                Ok::<_, hibana::EndpointError>(db)
                            },
                            async {
                                received_tick.recv::<Tick>().await?;
                                if visit == 1 {
                                    assert!(network.paused_send_is_waiting(0));
                                    network.resume_send();
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                for _ in 0..2 {
                                    let mut db = jobs.recv::<Loan>().await?;
                                    db.send::<Read>(&7).await?;
                                    assert_eq!(db.recv::<Value>().await?, 8);
                                    jobs.send::<Loan>(db).await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            sent_tick.send::<Tick>(&()),
                            async {
                                for _ in 0..2 {
                                    let value = service.recv::<Read>().await?;
                                    service.send::<Value>(&(value + 1)).await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                        )?;
                        db = returned;
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("the independent Tick must release a pending loan")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn nested_parallel_children_keep_the_enclosing_reentry_origin() {
    type Notice = g::Msg<238, ()>;
    type TickAck = g::Msg<239, ()>;
    let global = g::seq(
        g::send::<1, 0, Prefix>(),
        g::par(
            g::par(
                g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
                g::send::<0, 2, Notice>(),
            )
            .roll(),
            g::seq(g::send::<1, 0, Tick>(), g::send::<0, 1, TickAck>()),
        )
        .roll(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    let sid = SessionId::new(9286);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            assert!(
                a.par::<2>().is_err(),
                "the incoming prefix must complete first"
            );
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    b.send::<Prefix>(&()).await?;
                    a.recv::<Prefix>().await?;
                    for _ in 0..2 {
                        let [mut a_work, mut a_tick] = a.par()?;
                        let [mut b_work, mut b_tick] = b.par()?;
                        tokio::try_join!(
                            biased;
                            async {
                                for _ in 0..2 {
                                    let [mut exchange, mut notice] = a_work.par()?;
                                    tokio::try_join!(
                                        biased;
                                        async {
                                            for _ in 0..2 {
                                                exchange.send::<Work>(&()).await?;
                                                exchange.recv::<Ack>().await?;
                                            }
                                            Ok::<_, hibana::EndpointError>(())
                                        },
                                        notice.send::<Notice>(&()),
                                    )?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                a_tick.recv::<Tick>().await?;
                                a_tick.send::<TickAck>(&()).await
                            },
                            async {
                                for _ in 0..4 {
                                    b_work.recv::<Work>().await?;
                                    b_work.send::<Ack>(&()).await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                b_tick.send::<Tick>(&()).await?;
                                b_tick.recv::<TickAck>().await
                            },
                            async {
                                for _ in 0..2 {
                                    c.recv::<Notice>().await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                        )?;
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("nested parallel bodies stalled")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn nested_parallel_resolver_keeps_its_pending_delegation_choice() {
    use hibana::{
        Endpoint,
        runtime::resolver::{DecisionArm, ResolverRef},
    };
    type Loan = g::Msg<235, Endpoint<'static, 3>>;
    type Read = g::Msg<236, u32>;
    type Value = g::Msg<237, u32>;
    type OtherLoan = g::Msg<240, Endpoint<'static, 3>>;
    type OtherRead = g::Msg<241, u32>;
    type OtherValue = g::Msg<242, u32>;
    let global = g::seq(
        g::send::<1, 0, Prefix>(),
        g::par(
            g::route(
                g::seq(
                    g::send::<0, 1, Loan>(),
                    g::seq(
                        g::send::<3, 2, Read>(),
                        g::seq(g::send::<2, 3, Value>(), g::send::<1, 0, Loan>()),
                    ),
                ),
                g::seq(
                    g::send::<0, 1, OtherLoan>(),
                    g::seq(
                        g::send::<3, 2, OtherRead>(),
                        g::seq(g::send::<2, 3, OtherValue>(), g::send::<1, 0, OtherLoan>()),
                    ),
                ),
            )
            .resolve::<417>()
            .roll(),
            g::send::<1, 0, Tick>(),
        )
        .roll(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = common::TestTransport::new();
    let calls = core::cell::Cell::new(0usize);
    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
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
    rv0.set_resolver(
        &p0,
        ResolverRef::<417>::decision_state(&calls, |calls| {
            calls.set(calls.get() + 1);
            Ok(DecisionArm::Left)
        }),
    )
    .unwrap();
    let sid = SessionId::new(9287);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let mut db = rv0.enter(sid, &p3).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                for _ in 0..2 {
                    worker.send::<Prefix>(&()).await?;
                    owner.recv::<Prefix>().await?;
                    for visit in 0..2 {
                        let [mut loans, mut received_tick] = owner.par()?;
                        let [mut jobs, mut sent_tick] = worker.par()?;
                        if visit == 1 {
                            network.pause_send(0);
                        }
                        let (returned, (), (), (), ()) = tokio::try_join!(
                            biased;
                            async {
                                for _ in 0..2 {
                                    loans.send::<Loan>(db).await?;
                                    db = loans.recv::<Loan>().await?;
                                }
                                Ok::<_, hibana::EndpointError>(db)
                            },
                            async {
                                received_tick.recv::<Tick>().await?;
                                if visit == 1 {
                                    assert!(network.paused_send_is_waiting(0));
                                    network.resume_send();
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            async {
                                for _ in 0..2 {
                                    let branch = jobs.offer().await?;
                                    assert_eq!(branch.label(),235);
                                    drop(branch);
                                    let mut db = jobs.offer().await?.recv::<Loan>().await?;
                                    db.send::<Read>(&7).await?;
                                    assert_eq!(db.recv::<Value>().await?,8);
                                    jobs.send::<Loan>(db).await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                            sent_tick.send::<Tick>(&()),
                            async {
                                for _ in 0..2 {
                                    let branch = service.offer().await?;
                                    assert_eq!(branch.label(),236);
                                    drop(branch);
                                    let value = service.offer().await?.recv::<Read>().await?;
                                    service.send::<Value>(&(value+1)).await?;
                                }
                                Ok::<_, hibana::EndpointError>(())
                            },
                        )?;
                        db = returned;
                    }
                }
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("a pending loan cannot rerun its choice or block Tick")
            .unwrap();
        });
    assert_eq!(
        calls.get(),
        8,
        "one decision per loan, including pending sends"
    );
    assert!(network.queue_is_empty());
}

#[test]
fn a_received_endpoint_keeps_two_parallel_visits_distinct() {
    use hibana::Endpoint;
    type Loan = g::Msg<118, Endpoint<'static, 3>>;
    type Returned = g::Msg<119, Endpoint<'static, 3>>;
    type RequestA = g::Msg<120, ()>;
    type ReplyA = g::Msg<121, ()>;
    type RequestB = g::Msg<122, ()>;
    type ReplyB = g::Msg<123, ()>;
    type Tail = g::Msg<124, ()>;
    type TailReply = g::Msg<125, ()>;
    type Outside = g::Msg<126, ()>;
    type OutsideReply = g::Msg<127, ()>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::par(
                g::seq(
                    g::par(
                        g::seq(g::send::<3, 2, RequestA>(), g::send::<2, 3, ReplyA>()),
                        g::seq(g::send::<3, 2, RequestB>(), g::send::<2, 3, ReplyB>()).roll(),
                    )
                    .roll(),
                    g::seq(g::send::<3, 2, Tail>(), g::send::<2, 3, TailReply>()),
                ),
                g::seq(g::send::<3, 2, Outside>(), g::send::<2, 3, OutsideReply>()),
            ),
            g::send::<1, 0, Returned>(),
        ),
    );
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
    let sid = SessionId::new(9392);
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
                owner.send::<Loan>(db).await?;
                let mut db = worker.recv::<Loan>().await?;
                {
                    let [mut flow, mut outside] = db.par()?;
                    let [mut peer_flow, mut peer_outside] = service.par()?;
                    // The second visit completes its other arm first. Keeping
                    // the borrows distinguishes this cut from four repetitions
                    // of that arm in the first visit.
                    for order in [[0, 1, 1], [1, 1, 0]] {
                        let [mut first, mut repeated] = flow.par()?;
                        let [mut peer_first, mut peer_repeated] = peer_flow.par()?;
                        for part in order {
                            match part {
                                0 => {
                                    tokio::try_join!(
                                        first.send::<RequestA>(&()),
                                        peer_first.recv::<RequestA>()
                                    )?;
                                    tokio::try_join!(
                                        peer_first.send::<ReplyA>(&()),
                                        first.recv::<ReplyA>()
                                    )?;
                                }
                                1 => {
                                    tokio::try_join!(
                                        repeated.send::<RequestB>(&()),
                                        peer_repeated.recv::<RequestB>()
                                    )?;
                                    tokio::try_join!(
                                        peer_repeated.send::<ReplyB>(&()),
                                        repeated.recv::<ReplyB>()
                                    )?;
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    tokio::try_join!(flow.send::<Tail>(&()), peer_flow.recv::<Tail>())?;
                    tokio::try_join!(peer_flow.send::<TailReply>(&()), flow.recv::<TailReply>())?;
                    tokio::try_join!(outside.send::<Outside>(&()), peer_outside.recv::<Outside>())?;
                    tokio::try_join!(
                        peer_outside.send::<OutsideReply>(&()),
                        outside.recv::<OutsideReply>()
                    )?;
                }
                worker.send::<Returned>(db).await?;
                let _returned = owner.recv::<Returned>().await?;
                Ok::<_, hibana::EndpointError>(())
            })
            .await
            .expect("the scoped source visit must finish")
            .unwrap();
        });
    assert!(network.queue_is_empty());
}

#[test]
fn nested_parallel_delegation_preserves_a_child_when_its_parent_restarts_late() {
    use core::cell::Cell;
    use hibana::{
        Endpoint, EndpointError,
        runtime::resolver::{DecisionArm, ResolverRef},
    };
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type Notice = g::Msg<1, ()>;
    type Pulse = g::Msg<2, ()>;
    type Read = g::Msg<3, ()>;
    type Value = g::Msg<4, ()>;
    type Left = g::Msg<5, ()>;
    type LeftNotice = g::Msg<6, ()>;
    type LeftDone = g::Msg<7, ()>;
    type Continue = g::Msg<8, ()>;
    type Continued = g::Msg<9, ()>;
    type Right = g::Msg<10, ()>;
    type RightNotice = g::Msg<11, ()>;
    type Return<'r> = g::Msg<12, Endpoint<'r, 3>>;
    type OwnerRead = g::Msg<13, ()>;
    type OwnerValue = g::Msg<14, ()>;
    type Relend<'r> = g::Msg<15, Endpoint<'r, 3>>;
    type Finished<'r> = g::Msg<16, Endpoint<'r, 3>>;

    #[cfg(miri)]
    std::eprintln!("parallel-credit setup");
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::par(
                g::par(g::send::<0, 1, Notice>(), g::send::<1, 0, Pulse>().roll()),
                g::seq(
                    g::send::<3, 2, Read>(),
                    g::seq(
                        g::send::<2, 3, Value>(),
                        g::route(
                            g::seq(
                                g::send::<3, 2, Left>(),
                                g::seq(
                                    g::send::<3, 0, LeftNotice>(),
                                    g::seq(
                                        g::send::<3, 2, LeftDone>(),
                                        g::seq(
                                            g::send::<0, 1, Continue>(),
                                            g::send::<1, 0, Continued>(),
                                        ),
                                    ),
                                ),
                            ),
                            g::seq(
                                g::send::<3, 2, Right>(),
                                g::seq(
                                    g::send::<3, 0, RightNotice>(),
                                    g::seq(
                                        g::send::<1, 0, Return<'_>>(),
                                        g::seq(
                                            g::seq(
                                                g::send::<3, 2, OwnerRead>(),
                                                g::send::<2, 3, OwnerValue>(),
                                            ),
                                            g::send::<0, 1, Relend<'_>>(),
                                        ),
                                    ),
                                ),
                            ),
                        )
                        .resolve::<0>(),
                    ),
                )
                .roll(),
            )
            .roll()
            .roll(),
            g::send::<1, 0, Finished<'_>>(),
        ),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let cases = match std::env::var("HIBANA_PARALLEL_CREDIT_CASE") {
        Ok(value) => {
            let case: usize = value
                .parse()
                .expect("parallel credit case must be an integer");
            assert!(case < 16, "parallel credit case must be in 0..16");
            case..case + 1
        }
        Err(std::env::VarError::NotPresent) => 0..16,
        Err(std::env::VarError::NotUnicode(_)) => panic!("parallel credit case must be Unicode"),
    };
    for right_visit in 0..8 {
        for reverse in [false, true] {
            let case = right_visit * 2 + usize::from(reverse);
            if !cases.contains(&case) {
                continue;
            }
            #[cfg(miri)]
            std::eprintln!("parallel-credit start: right_visit={right_visit} reverse={reverse}");
            let network = common::TestTransport::new();
            let arm = Cell::new(DecisionArm::Left);
            let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
            let mut s0 = SessionKitStorage::uninit();
            let mut s1 = SessionKitStorage::uninit();
            let mut s2 = SessionKitStorage::uninit();
            let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
            let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
            let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
            rv1.set_resolver(
                &p1,
                ResolverRef::<0>::decision_state(&arm, |arm| Ok(arm.get())),
            )
            .unwrap();
            let sid = SessionId::new(9409);
            let mut owner = rv0.enter(sid, &p0).unwrap();
            let db = rv0.enter(sid, &p3).unwrap();
            let mut worker = rv1.enter(sid, &p1).unwrap();
            let mut service = rv2.enter(sid, &p2).unwrap();
            let owner_run = async {
                owner.send::<Loan<'_>>(db).await?;
                for _ in 0..2 {
                    let [mut notice, mut pulses, mut coordination] = owner.par()?;
                    for _ in 0..2 {
                        tokio::try_join!(
                            async {
                                tokio::task::yield_now().await;
                                tokio::task::yield_now().await;
                                notice.send::<Notice>(&()).await
                            },
                            async {
                                for _ in 0..2 {
                                    pulses.recv::<Pulse>().await?;
                                }
                                Ok::<_, EndpointError>(())
                            },
                            async {
                                for _ in 0..2 {
                                    let branch = coordination.offer().await?;
                                    match branch.label() {
                                        6 => {
                                            branch.recv::<LeftNotice>().await?;
                                            coordination.send::<Continue>(&()).await?;
                                            coordination.recv::<Continued>().await?;
                                        }
                                        11 => {
                                            branch.recv::<RightNotice>().await?;
                                            let mut db = coordination.recv::<Return<'_>>().await?;
                                            db.send::<OwnerRead>(&()).await?;
                                            db.recv::<OwnerValue>().await?;
                                            coordination.send::<Relend<'_>>(db).await?;
                                        }
                                        label => panic!("unexpected owner branch {label}"),
                                    }
                                }
                                Ok::<_, EndpointError>(())
                            },
                        )?;
                    }
                }
                let _db = owner.recv::<Finished<'_>>().await?;
                Ok::<_, EndpointError>(())
            };
            let worker_run = async {
                let mut db = worker.recv::<Loan<'_>>().await?;
                for outer in 0..2 {
                    let [mut notice, mut pulses, mut coordination] = worker.par()?;
                    for inner in 0..2 {
                        let (_, _, returned) = tokio::try_join!(
                            notice.recv::<Notice>(),
                            async {
                                for _ in 0..2 {
                                    tokio::task::yield_now().await;
                                    pulses.send::<Pulse>(&()).await?;
                                }
                                Ok::<_, EndpointError>(())
                            },
                            async {
                                let mut db = db;
                                for visit in 0..2 {
                                    db.send::<Read>(&()).await?;
                                    db.recv::<Value>().await?;
                                    arm.set(if outer * 4 + inner * 2 + visit == right_visit {
                                        DecisionArm::Right
                                    } else {
                                        DecisionArm::Left
                                    });
                                    let branch = db.offer().await?;
                                    match branch.label() {
                                        5 => {
                                            branch.send::<Left>(&()).await?;
                                            db.send::<LeftNotice>(&()).await?;
                                            db.send::<LeftDone>(&()).await?;
                                            coordination.recv::<Continue>().await?;
                                            coordination.send::<Continued>(&()).await?;
                                        }
                                        10 => {
                                            branch.send::<Right>(&()).await?;
                                            db.send::<RightNotice>(&()).await?;
                                            coordination.send::<Return<'_>>(db).await?;
                                            db = coordination.recv::<Relend<'_>>().await?;
                                        }
                                        label => panic!("unexpected worker branch {label}"),
                                    }
                                }
                                Ok::<_, EndpointError>(db)
                            },
                        )?;
                        db = returned;
                    }
                }
                worker.send::<Finished<'_>>(db).await?;
                Ok::<_, EndpointError>(())
            };
            let service_run = async {
                for _ in 0..8 {
                    service.recv::<Read>().await?;
                    service.send::<Value>(&()).await?;
                    let branch = service.offer().await?;
                    match branch.label() {
                        5 => {
                            branch.recv::<Left>().await?;
                            service.recv::<LeftDone>().await?;
                        }
                        10 => {
                            branch.recv::<Right>().await?;
                            service.recv::<OwnerRead>().await?;
                            service.send::<OwnerValue>(&()).await?;
                        }
                        label => panic!("unexpected service branch {label}"),
                    }
                }
                Ok::<_, EndpointError>(())
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(async {
                    tokio::time::timeout(core::time::Duration::from_secs(1), async {
                        if reverse {
                            tokio::try_join!(service_run, worker_run, owner_run)?;
                        } else {
                            tokio::try_join!(owner_run, worker_run, service_run)?;
                        }
                        Ok::<_, EndpointError>(())
                    })
                    .await
                    .expect("independent repeated fragments stalled")
                    .unwrap();
                });
            assert!(network.queue_is_empty());
            #[cfg(miri)]
            std::eprintln!("parallel-credit complete: right_visit={right_visit} reverse={reverse}");
        }
    }
}
