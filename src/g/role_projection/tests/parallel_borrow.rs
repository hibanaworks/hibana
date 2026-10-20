use super::*;
mod associative;
mod choice;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn an_owned_continuation_returns_while_the_carrier_drives_an_independent_arm() {
    type Loan = g::Msg<91, Endpoint<'static, 10>>;
    type Read = g::Msg<92, u32>;
    type Value = g::Msg<93, u32>;
    type Ping = g::Msg<94, u32>;
    type Pong = g::Msg<95, u32>;
    type LoanBody = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<g::Send<10, 3, Read>, g::Seq<g::Send<3, 10, Value>, g::Send<1, 0, Loan>>>,
    >;
    type Side = g::Seq<g::Send<0, 2, Ping>, g::Send<2, 0, Pong>>;
    type Steps =
        g::Seq<g::Par<LoanBody, Side>, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let side_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut side_slab = [0; 32 * 1024];
    let mut service_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut side_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    let side_rv = side_storage
        .init()
        .rendezvous(&mut side_slab, network.clone())
        .unwrap();
    let service_rv = service_storage
        .init()
        .rendezvous(&mut service_slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1640);
    let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
    let db = owner_rv.enter(sid, &db_program).unwrap();
    let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
    let mut side = side_rv.enter(sid, &side_program).unwrap();
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        let mut db = {
                            let [mut loan, mut independent] = owner.par().unwrap();
                            let (db, ()) = tokio::join!(
                                async {
                                    loan.send::<Loan>(db).await.unwrap();
                                    loan.recv::<Loan>().await.unwrap()
                                },
                                async {
                                    independent.send::<Ping>(&5).await.unwrap();
                                    assert_eq!(independent.recv::<Pong>().await.unwrap(), 6);
                                },
                            );
                            db
                        };
                        db.send::<Read>(&42).await.unwrap();
                        assert_eq!(db.recv::<Value>().await.unwrap(), 43);
                    },
                    async {
                        let mut db = worker.recv::<Loan>().await.unwrap();
                        db.send::<Read>(&7).await.unwrap();
                        assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                        worker.send::<Loan>(db).await.unwrap();
                    },
                    async {
                        let value = side.recv::<Ping>().await.unwrap();
                        side.send::<Pong>(&(value + 1)).await.unwrap();
                    },
                    async {
                        let value = service.recv::<Read>().await.unwrap();
                        service.send::<Value>(&(value + 1)).await.unwrap();
                        let value = service.recv::<Read>().await.unwrap();
                        service.send::<Value>(&(value + 1)).await.unwrap();
                    },
                );
            })
            .await
            .expect("loan and independent carrier arm must both progress");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_received_endpoint_drives_its_parallel_arms_and_returns_its_continuation() {
    type Loan = g::Msg<101, Endpoint<'static, 10>>;
    type A = g::Msg<102, u32>;
    type AR = g::Msg<103, u32>;
    type B = g::Msg<104, u32>;
    type BR = g::Msg<105, u32>;
    type Final = g::Msg<106, u32>;
    type FinalReply = g::Msg<107, u32>;
    type Parallel = g::Par<
        g::Seq<g::Send<10, 3, A>, g::Send<3, 10, AR>>,
        g::Seq<g::Send<10, 3, B>, g::Send<3, 10, BR>>,
    >;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            Parallel,
            g::Seq<g::Send<1, 0, Loan>, g::Seq<g::Send<10, 3, Final>, g::Send<3, 10, FinalReply>>>,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut service_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    let service_rv = service_storage
        .init()
        .rendezvous(&mut service_slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1645);
    let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
    let db = owner_rv.enter(sid, &db_program).unwrap();
    let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        owner.send::<Loan>(db).await.unwrap();
                        let mut db = owner.recv::<Loan>().await.unwrap();
                        db.send::<Final>(&42).await.unwrap();
                        assert_eq!(db.recv::<FinalReply>().await.unwrap(), 43);
                    },
                    async {
                        let mut db = worker.recv::<Loan>().await.unwrap();
                        {
                            let [mut left, mut right] = db.par().unwrap();
                            tokio::join!(
                                async {
                                    left.send::<A>(&7).await.unwrap();
                                    assert_eq!(left.recv::<AR>().await.unwrap(), 8);
                                },
                                async {
                                    right.send::<B>(&11).await.unwrap();
                                    assert_eq!(right.recv::<BR>().await.unwrap(), 12);
                                },
                            );
                        }
                        worker.send::<Loan>(db).await.unwrap();
                    },
                    async {
                        {
                            let [mut left, mut right] = service.par().unwrap();
                            tokio::join!(
                                async {
                                    let value = left.recv::<A>().await.unwrap();
                                    left.send::<AR>(&(value + 1)).await.unwrap();
                                },
                                async {
                                    let value = right.recv::<B>().await.unwrap();
                                    right.send::<BR>(&(value + 1)).await.unwrap();
                                },
                            );
                        }
                        let value = service.recv::<Final>().await.unwrap();
                        service.send::<FinalReply>(&(value + 1)).await.unwrap();
                    },
                );
            })
            .await
            .expect("a received Endpoint must keep its own parallel semantics and continuation");
        });
    assert!(network.queue_is_empty());
}
