//! Initial resource issuance must happen at its source-assigned actor.
#[path = "../examples/support/in_memory.rs"]
mod in_memory;

use hibana::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use in_memory::InMemoryTransport;

#[test]
fn returning_an_endpoint_before_its_work_reports_a_protocol_order_error() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
            type Read = g::Msg<2, u32>;
            let global = g::seq(
                g::send::<0, 1, Loan<'_>>(),
                g::seq(g::send::<2, 3, Read>(), g::send::<1, 0, Loan<'_>>()),
            );
            let owner_program = project::<0>(&global);
            let worker_program = project::<1>(&global);
            let resource_program = project::<2>(&global);
            let sid = SessionId::new(9086);
            // SAFETY: one physical test resource, with no other initial issuer.
            let network = InMemoryTransport::new([((sid, 2), unsafe { EndpointBinding::new() })]);
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage.init().rendezvous(&mut slab, network).unwrap();
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &resource_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            owner.send::<Loan<'_>>(db).await.unwrap();
            let db = worker.recv::<Loan<'_>>().await.unwrap();
            let error = worker
                .send::<Loan<'_>>(db)
                .await
                .expect_err("Read must precede return");
            assert!(format!("{error:?}").contains("OutOfOrder"), "{error:?}");
            let error = owner
                .recv::<Loan<'_>>()
                .await
                .err()
                .expect("the local run must fault");
            assert!(
                format!("{error:?}").contains("ProtocolViolation"),
                "{error:?}"
            );
        });
}

#[test]
fn concurrent_sessions_can_bind_distinct_resources_with_the_same_role_number() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
            type Read = g::Msg<2, u32>;
            type Value = g::Msg<3, u32>;
            let global = g::seq(
                g::send::<0, 1, Loan<'_>>(),
                g::seq(
                    g::send::<2, 3, Read>(),
                    g::seq(g::send::<3, 2, Value>(), g::send::<1, 0, Loan<'_>>()),
                ),
            );
            let owner_program = project::<0>(&global);
            let worker_program = project::<1>(&global);
            let resource_program = project::<2>(&global);
            let service_program = project::<3>(&global);
            let first = SessionId::new(9084);
            let second = SessionId::new(9085);
            // SAFETY: these are two distinct physical resource boundaries, not two
            // issuers for the same resource. Both runs finish before their bindings.
            let network = InMemoryTransport::new(unsafe {
                [
                    ((first, 2), EndpointBinding::new()),
                    ((second, 2), EndpointBinding::new()),
                ]
            });
            let mut slab = [0; 128 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage.init().rendezvous(&mut slab, network).unwrap();
            let mut owner1 = rv.enter(first, &owner_program).unwrap();
            let db1 = rv.enter(first, &resource_program).unwrap();
            let mut worker1 = rv.enter(first, &worker_program).unwrap();
            let mut service1 = rv.enter(first, &service_program).unwrap();
            let mut owner2 = rv.enter(second, &owner_program).unwrap();
            let db2 = rv.enter(second, &resource_program).unwrap();
            let mut worker2 = rv.enter(second, &worker_program).unwrap();
            let mut service2 = rv.enter(second, &service_program).unwrap();
            assert!(rv.enter(first, &resource_program).is_err());
            assert!(rv.enter(second, &resource_program).is_err());
            tokio::join!(
                async {
                    owner1.send::<Loan<'_>>(db1).await.unwrap();
                    drop(owner1.recv::<Loan<'_>>().await.unwrap());
                },
                async {
                    owner2.send::<Loan<'_>>(db2).await.unwrap();
                    drop(owner2.recv::<Loan<'_>>().await.unwrap());
                },
                async {
                    let mut db = worker1.recv::<Loan<'_>>().await.unwrap();
                    db.send::<Read>(&7).await.unwrap();
                    assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                    worker1.send::<Loan<'_>>(db).await.unwrap();
                },
                async {
                    let mut db = worker2.recv::<Loan<'_>>().await.unwrap();
                    db.send::<Read>(&41).await.unwrap();
                    assert_eq!(db.recv::<Value>().await.unwrap(), 1041);
                    worker2.send::<Loan<'_>>(db).await.unwrap();
                },
                async {
                    let key = service1.recv::<Read>().await.unwrap();
                    service1.send::<Value>(&(key + 1)).await.unwrap();
                },
                async {
                    let key = service2.recv::<Read>().await.unwrap();
                    service2.send::<Value>(&(key + 1000)).await.unwrap();
                }
            );
        });
}

#[test]
fn a_wrong_or_missing_initial_actor_cannot_consume_the_resource_binding() {
    const OWNER: u8 = 0;
    const WORKER: u8 = 1;
    const DB: u8 = 2;
    const SERVICE: u8 = 3;
    type Loan<'r> = g::Msg<1, Endpoint<'r, DB>>;
    type Read = g::Msg<2, u32>;
    type Value = g::Msg<3, u32>;
    let global = g::seq(
        g::send::<OWNER, WORKER, Loan<'_>>(),
        g::seq(
            g::send::<DB, SERVICE, Read>(),
            g::seq(
                g::send::<SERVICE, DB, Value>(),
                g::seq(
                    g::send::<WORKER, OWNER, Loan<'_>>(),
                    g::seq(
                        g::send::<DB, SERVICE, Read>(),
                        g::send::<SERVICE, DB, Value>(),
                    ),
                ),
            ),
        ),
    );
    let owner_program = project::<OWNER>(&global);
    let worker_program = project::<WORKER>(&global);
    let db_program = project::<DB>(&global);
    let service_program = project::<SERVICE>(&global);
    // SAFETY: this test is the sole issuer for this DB, and every initial
    // attachment path below uses this same binding. No remote right survives it.
    let sid = SessionId::new(9080);
    let network = InMemoryTransport::new([((sid, DB), unsafe { EndpointBinding::new() })]);
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut service_slab = [0; 32 * 1024];
    let mut empty_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let mut empty_storage = SessionKitStorage::uninit();
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
    let empty_rv = empty_storage
        .init()
        .rendezvous(&mut empty_slab, network)
        .unwrap();
    assert!(
        empty_rv.enter(sid, &db_program).is_err(),
        "a runtime without the initial actor must reject resource issuance"
    );
    let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
    assert!(
        worker_rv.enter(sid, &db_program).is_err(),
        "the future recipient is not the initial issuer"
    );
    let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
    assert!(
        owner_rv.enter(SessionId::new(9081), &db_program).is_err(),
        "an initial actor in another session does not authorize this issuance"
    );
    let db = owner_rv.enter(sid, &db_program).unwrap();
    assert!(owner_rv.enter(sid, &db_program).is_err());
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    let (owner_value, worker_value, ()) = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            tokio::join!(
                async {
                    owner.send::<Loan<'_>>(db).await.unwrap();
                    let mut db = owner.recv::<Loan<'_>>().await.unwrap();
                    db.send::<Read>(&41).await.unwrap();
                    db.recv::<Value>().await.unwrap()
                },
                async {
                    let mut db = worker.recv::<Loan<'_>>().await.unwrap();
                    db.send::<Read>(&7).await.unwrap();
                    let value = db.recv::<Value>().await.unwrap();
                    worker.send::<Loan<'_>>(db).await.unwrap();
                    value
                },
                async {
                    for _ in 0..2 {
                        let key = service.recv::<Read>().await.unwrap();
                        service.send::<Value>(&(key + 1)).await.unwrap();
                    }
                }
            )
        });
    assert_eq!((owner_value, worker_value), (42, 8));
}

#[test]
fn an_initial_actor_that_was_dropped_does_not_authorize_resource_issuance() {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::send::<2, 3, g::Msg<2, u32>>(),
    );
    let owner_program = project::<0>(&global);
    let db_program = project::<2>(&global);
    // SAFETY: this integration test constructs the only initial authority.
    let fresh = SessionId::new(9083);
    let network = InMemoryTransport::new([((fresh, 2), unsafe { EndpointBinding::new() })]);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, network).unwrap();
    let sid = SessionId::new(9082);
    let owner = rv.enter(sid, &owner_program).unwrap();
    drop(owner);
    assert!(rv.enter(sid, &db_program).is_err());
    // No resource view was issued. Failed setup must leave its original
    // authority available to the correctly attached actor in a fresh run.
    let _owner = rv.enter(fresh, &owner_program).unwrap();
    let _db = rv.enter(fresh, &db_program).unwrap();
}
