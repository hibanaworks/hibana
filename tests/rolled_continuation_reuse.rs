//! Elastic reentry must not share an outbound selector with a continuation.
use hibana::g::{self, Msg, ProjectionProblem};

#[path = "../examples/support/in_memory.rs"]
mod in_memory;

#[test]
fn a_distinct_outbound_continuation_keeps_the_completed_roll_available() {
    use hibana::runtime::{SessionKitStorage, ids::SessionId, program::project};
    type Ping = Msg<1, u32>;
    type Switch = Msg<2, ()>;
    let global = g::seq(g::send::<0, 1, Ping>().roll(), g::send::<0, 1, Switch>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let mut slab = [0; 65536];
    let mut storage = SessionKitStorage::uninit();
    let runtime = storage
        .init()
        .rendezvous(&mut slab, in_memory::InMemoryTransport::new([]))
        .unwrap();
    let session = SessionId::new(9018);
    let mut left = runtime.enter(session, &p0).unwrap();
    let mut right = runtime.enter(session, &p1).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::join!(
                    async {
                        left.send::<Ping>(&1).await.unwrap();
                        left.send::<Switch>(&()).await.unwrap();
                        left.send::<Ping>(&2).await.unwrap();
                    },
                    async {
                        assert_eq!(right.recv::<Ping>().await.unwrap(), 1);
                        right.recv::<Switch>().await.unwrap();
                        assert_eq!(right.recv::<Ping>().await.unwrap(), 2);
                    },
                );
                assert!(
                    left.send::<Switch>(&()).await.is_err(),
                    "repeating Ping must not reset the one-shot continuation"
                );
            })
            .await
            .expect("a completed roll remains repeatable after the continuation");
        });
}

#[test]
fn an_unrolled_prefix_cannot_restart_after_its_continuation() {
    use hibana::runtime::{SessionKitStorage, ids::SessionId, program::project};
    type First = Msg<1, ()>;
    type Second = Msg<2, ()>;
    let global = g::seq(g::send::<0, 1, First>(), g::send::<0, 1, Second>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let mut slab = [0; 65536];
    let mut storage = SessionKitStorage::uninit();
    let runtime = storage
        .init()
        .rendezvous(&mut slab, in_memory::InMemoryTransport::new([]))
        .unwrap();
    let session = SessionId::new(9019);
    let mut left = runtime.enter(session, &p0).unwrap();
    let mut right = runtime.enter(session, &p1).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                left.send::<First>(&()).await.unwrap();
                right.recv::<First>().await.unwrap();
                left.send::<Second>(&()).await.unwrap();
                right.recv::<Second>().await.unwrap();
                assert!(left.send::<First>(&()).await.is_err());
            })
            .await
            .expect("both one-shot messages must finish");
        });
}

#[test]
fn an_acquired_view_guards_repeated_requests_before_the_owner_continuation() {
    use hibana::{
        Endpoint,
        runtime::{
            SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding,
        },
    };
    type Loan<'r> = Msg<1, Endpoint<'r, 2>>;
    type Read = Msg<2, u32>;
    type Value = Msg<3, u32>;
    let work = g::seq(g::send::<2, 3, Read>(), g::send::<3, 2, Value>());
    let cycle = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(work, g::send::<1, 0, Loan<'_>>()),
    );
    let global = g::seq(cycle.roll(), work);
    assert_eq!(g::diagnose(&global), None);
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let db_program = project::<2>(&global);
    let service_program = project::<3>(&global);
    for reverse in [false, true] {
        let sid = SessionId::new(9017);
        // SAFETY: one initial issuer for this test's physical resource.
        let network =
            in_memory::InMemoryTransport::new([((sid, 2), unsafe { EndpointBinding::new() })]);
        let [mut owner_slab, mut worker_slab, mut service_slab] = [(); 3].map(|_| vec![0; 32768]);
        let (mut os, mut ws, mut ss) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let owner_runtime = os
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let worker_runtime = ws
            .init()
            .rendezvous(&mut worker_slab, network.clone())
            .unwrap();
        let service_runtime = ss.init().rendezvous(&mut service_slab, network).unwrap();
        let mut owner = owner_runtime.enter(sid, &owner_program).unwrap();
        let mut db = owner_runtime.enter(sid, &db_program).unwrap();
        let mut worker = worker_runtime.enter(sid, &worker_program).unwrap();
        let mut service = service_runtime.enter(sid, &service_program).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                let owner_run = async {
                    for _ in 0..2 {
                        owner.send::<Loan<'_>>(db).await.unwrap();
                        db = owner.recv::<Loan<'_>>().await.unwrap();
                    }
                    db.send::<Read>(&99).await.unwrap();
                    assert_eq!(db.recv::<Value>().await.unwrap(), 109);
                };
                let worker_run = async {
                    for value in [11, 12] {
                        let mut db = worker.recv::<Loan<'_>>().await.unwrap();
                        db.send::<Read>(&value).await.unwrap();
                        assert_eq!(db.recv::<Value>().await.unwrap(), value + 10);
                        worker.send::<Loan<'_>>(db).await.unwrap();
                    }
                };
                let service_run = async {
                    for expected in [11, 12, 99] {
                        let value = service.recv::<Read>().await.unwrap();
                        assert_eq!(value, expected);
                        service.send::<Value>(&(value + 10)).await.unwrap();
                    }
                };
                tokio::time::timeout(core::time::Duration::from_secs(1), async {
                    if reverse {
                        tokio::join!(biased; service_run, worker_run, owner_run);
                    } else {
                        tokio::join!(biased; owner_run, worker_run, service_run);
                    }
                })
                .await
                .expect("both repeated loans and the owner's continuation must finish");
            });
    }
}

#[test]
fn inbound_boundary_does_not_disambiguate_later_outbound_reentry() {
    let protocol = g::seq(
        g::send::<0, 1, Msg<3, ()>>().roll(),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    let diagnostic = g::diagnose(&protocol).expect("both outbound occurrences remain eligible");
    assert_eq!(diagnostic.problem, ProjectionProblem::ReentrySelector);
    assert_eq!(diagnostic.role, Some(0));
    assert_eq!(diagnostic.first.unwrap().index, 0);
    assert_eq!(diagnostic.second.unwrap().index, 2);
}

#[test]
fn a_role_erases_unrelated_prefixes_before_reentry() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn a_role_erases_an_unrelated_prefix_before_an_inbound_boundary() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<3, 2, Msg<24, ()>>(),
            g::send::<2, 3, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn distinct_reentry_contracts_remain_projectable() {
    let protocol = g::seq(
        g::send::<0, 1, Msg<3, ()>>().roll(),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<13, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn ordinary_sequential_reuse_remains_projectable() {
    let protocol = g::seq(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<3, ()>>());
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn an_inbound_head_distinguishes_later_same_label_replies() {
    let protocol = g::seq(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<1, 0, Msg<3, ()>>(),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<1, 0, Msg<3, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn exclusive_route_arms_can_reuse_an_outbound_selector() {
    let protocol = g::route(
        g::seq(
            g::send::<0, 1, Msg<10, ()>>(),
            g::send::<0, 1, Msg<3, ()>>().roll(),
        ),
        g::seq(
            g::send::<0, 1, Msg<20, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    assert_eq!(g::diagnose(&protocol), None);
}

#[test]
fn a_parallel_join_does_not_hide_a_competing_continuation() {
    let protocol = g::seq(
        g::par(
            g::send::<0, 1, Msg<3, ()>>().roll(),
            g::send::<2, 3, Msg<10, ()>>(),
        ),
        g::seq(
            g::send::<1, 0, Msg<24, ()>>(),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
    );
    assert_eq!(
        g::diagnose(&protocol).map(|d| d.problem),
        Some(ProjectionProblem::ReentrySelector)
    );
}

#[test]
fn independent_parallel_rolls_remain_projectable() {
    let protocol = g::par(
        g::seq(
            g::send::<0, 1, Msg<3, ()>>().roll(),
            g::send::<1, 0, Msg<24, ()>>(),
        ),
        g::send::<2, 3, Msg<3, ()>>().roll(),
    );
    assert_eq!(g::diagnose(&protocol), None);
}
