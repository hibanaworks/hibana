mod common;
#[path = "../examples/support/in_memory.rs"]
mod in_memory;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

#[test]
fn nested_choice_remains_available_after_its_outer_parallel_sibling_finishes() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            use hibana::{
                Endpoint,
                runtime::{
                    resolver::{DecisionArm, ResolverRef},
                    transport::EndpointBinding,
                },
            };

            type Loan<'r> = Msg<0, Endpoint<'r, 3>>;
            type Return<'r> = Msg<3, Endpoint<'r, 3>>;
            let loan = g::seq(
                g::send::<0, 1, Loan<'_>>(),
                g::seq(
                    g::send::<3, 2, Msg<1, ()>>(),
                    g::seq(g::send::<2, 3, Msg<2, ()>>(), g::send::<1, 0, Return<'_>>()),
                ),
            );
            let choice = g::route(
                g::seq(
                    g::send::<0, 1, Msg<4, ()>>(),
                    g::seq(g::send::<0, 2, Msg<5, ()>>(), g::send::<1, 2, Msg<6, ()>>()),
                ),
                g::seq(
                    g::send::<0, 1, Msg<7, ()>>(),
                    g::seq(
                        g::send::<1, 2, Msg<8, ()>>(),
                        g::seq(
                            g::send::<2, 1, Msg<9, ()>>(),
                            g::send::<1, 0, Msg<10, ()>>(),
                        ),
                    ),
                ),
            );
            let sibling = g::seq(
                g::send::<0, 1, Msg<11, ()>>(),
                g::seq(
                    g::send::<1, 2, Msg<12, ()>>(),
                    g::seq(
                        g::send::<2, 1, Msg<13, ()>>(),
                        g::send::<1, 0, Msg<14, ()>>(),
                    ),
                ),
            );
            let global = g::route(
                g::par(g::par(loan, choice), sibling),
                g::seq(
                    g::send::<0, 1, Msg<15, Endpoint<'_, 3>>>(),
                    g::seq(
                        g::send::<3, 2, Msg<16, ()>>(),
                        g::seq(
                            g::send::<2, 3, Msg<17, ()>>(),
                            g::send::<1, 0, Msg<18, Endpoint<'_, 3>>>(),
                        ),
                    ),
                ),
            )
            .resolve::<0>();
            let (p0, p1, p2, p3) = (
                project::<0>(&global),
                project::<1>(&global),
                project::<2>(&global),
                project::<3>(&global),
            );
            let sid = SessionId::new(87);
            // SAFETY: this test owns one resource and its sole initial binding.
            let transport =
                in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
            let (mut slab0, mut slab1, mut slab2) =
                (vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]);
            let (mut s0, mut s1, mut s2) = (
                SessionKitStorage::uninit(),
                SessionKitStorage::uninit(),
                SessionKitStorage::uninit(),
            );
            let r0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
            let r1 = s1.init().rendezvous(&mut slab1, transport.clone()).unwrap();
            let r2 = s2.init().rendezvous(&mut slab2, transport).unwrap();
            r0.set_resolver(
                &p0,
                ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Left)),
            )
            .unwrap();
            let mut owner = r0.enter(sid, &p0).unwrap();
            let mut worker = r1.enter(sid, &p1).unwrap();
            let mut service = r2.enter(sid, &p2).unwrap();
            let db = r0.enter(sid, &p3).unwrap();

            owner.send::<Msg<11, ()>>(&()).await.unwrap();
            worker
                .offer()
                .await
                .unwrap()
                .recv::<Msg<11, ()>>()
                .await
                .unwrap();
            worker.send::<Msg<12, ()>>(&()).await.unwrap();
            service.recv::<Msg<12, ()>>().await.unwrap();
            service.send::<Msg<13, ()>>(&()).await.unwrap();
            worker.recv::<Msg<13, ()>>().await.unwrap();
            worker.send::<Msg<14, ()>>(&()).await.unwrap();
            owner.recv::<Msg<14, ()>>().await.unwrap();

            owner.send::<Msg<4, ()>>(&()).await.unwrap();
            let branch = worker.offer().await.unwrap();
            assert_eq!(branch.label(), 4);
            drop(branch);
            worker
                .offer()
                .await
                .unwrap()
                .recv::<Msg<4, ()>>()
                .await
                .unwrap();
            owner.send::<Msg<5, ()>>(&()).await.unwrap();
            service.recv::<Msg<5, ()>>().await.unwrap();
            worker.send::<Msg<6, ()>>(&()).await.unwrap();
            service.recv::<Msg<6, ()>>().await.unwrap();

            owner.send::<Loan<'_>>(db).await.unwrap();
            let mut db = worker.recv::<Loan<'_>>().await.unwrap();
            db.send::<Msg<1, ()>>(&()).await.unwrap();
            service.recv::<Msg<1, ()>>().await.unwrap();
            service.send::<Msg<2, ()>>(&()).await.unwrap();
            db.recv::<Msg<2, ()>>().await.unwrap();
            worker.send::<Return<'_>>(db).await.unwrap();
            let _db = owner.recv::<Return<'_>>().await.unwrap();
        });
}

#[test]
fn parallel_choice_is_observable_before_a_sequential_resolver() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            use hibana::{
                Endpoint,
                runtime::{
                    resolver::{DecisionArm, ResolverRef},
                    transport::EndpointBinding,
                },
            };

            let choice = g::route(
                g::seq(
                    g::send::<0, 1, Msg<0, ()>>(),
                    g::seq(g::send::<0, 2, Msg<1, ()>>(), g::send::<1, 2, Msg<2, ()>>()),
                ),
                g::seq(
                    g::send::<0, 1, Msg<3, ()>>(),
                    g::seq(g::send::<0, 2, Msg<4, ()>>(), g::send::<1, 2, Msg<5, ()>>()),
                ),
            );
            let loan = g::seq(
                g::send::<0, 1, Msg<6, Endpoint<'_, 3>>>(),
                g::seq(
                    g::send::<3, 2, Msg<7, ()>>(),
                    g::seq(
                        g::send::<2, 3, Msg<8, ()>>(),
                        g::send::<1, 0, Msg<9, Endpoint<'_, 3>>>(),
                    ),
                ),
            );
            let next = g::route(
                g::seq(
                    g::send::<0, 1, Msg<10, Endpoint<'_, 3>>>(),
                    g::seq(
                        g::send::<3, 2, Msg<11, ()>>(),
                        g::seq(
                            g::send::<2, 3, Msg<12, ()>>(),
                            g::send::<1, 0, Msg<13, Endpoint<'_, 3>>>(),
                        ),
                    ),
                ),
                g::seq(
                    g::send::<0, 1, Msg<14, Endpoint<'_, 3>>>(),
                    g::seq(
                        g::send::<3, 2, Msg<15, ()>>(),
                        g::seq(
                            g::send::<2, 3, Msg<16, ()>>(),
                            g::send::<1, 0, Msg<17, Endpoint<'_, 3>>>(),
                        ),
                    ),
                ),
            )
            .resolve::<0>();
            let global = g::seq(g::par(choice, loan), next);
            let (p0, p1, p2, p3) = (
                project::<0>(&global),
                project::<1>(&global),
                project::<2>(&global),
                project::<3>(&global),
            );
            let sid = SessionId::new(88);
            let calls = core::cell::Cell::new(0);
            // SAFETY: this test owns one resource and its sole initial binding.
            let transport =
                in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
            let (mut slab0, mut slab1, mut slab2) =
                (vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]);
            let (mut s0, mut s1, mut s2) = (
                SessionKitStorage::uninit(),
                SessionKitStorage::uninit(),
                SessionKitStorage::uninit(),
            );
            let r0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
            let r1 = s1.init().rendezvous(&mut slab1, transport.clone()).unwrap();
            let r2 = s2.init().rendezvous(&mut slab2, transport).unwrap();
            r0.set_resolver(
                &p0,
                ResolverRef::<0>::decision_state(&calls, |calls| {
                    calls.set(calls.get() + 1);
                    Ok(DecisionArm::Left)
                }),
            )
            .unwrap();
            let mut owner = r0.enter(sid, &p0).unwrap();
            let mut worker = r1.enter(sid, &p1).unwrap();
            let mut service = r2.enter(sid, &p2).unwrap();
            let db = r0.enter(sid, &p3).unwrap();

            owner.send::<Msg<6, Endpoint<'_, 3>>>(db).await.unwrap();
            let mut db = worker.recv::<Msg<6, Endpoint<'_, 3>>>().await.unwrap();
            db.send::<Msg<7, ()>>(&()).await.unwrap();
            service.recv::<Msg<7, ()>>().await.unwrap();
            service.send::<Msg<8, ()>>(&()).await.unwrap();
            db.recv::<Msg<8, ()>>().await.unwrap();
            worker.send::<Msg<9, Endpoint<'_, 3>>>(db).await.unwrap();
            let db = owner.recv::<Msg<9, Endpoint<'_, 3>>>().await.unwrap();
            owner.send::<Msg<0, ()>>(&()).await.unwrap();
            assert_eq!(calls.get(), 0, "the join has not completed");
            let branch =
                tokio::time::timeout(core::time::Duration::from_millis(100), worker.offer())
                    .await
                    .expect("the queued parallel choice must be observable")
                    .unwrap();
            assert_eq!(branch.label(), 0);
            branch.recv::<Msg<0, ()>>().await.unwrap();
            owner.send::<Msg<1, ()>>(&()).await.unwrap();
            service.recv::<Msg<1, ()>>().await.unwrap();
            worker.send::<Msg<2, ()>>(&()).await.unwrap();
            service.recv::<Msg<2, ()>>().await.unwrap();
            assert_eq!(calls.get(), 0);

            owner.send::<Msg<10, Endpoint<'_, 3>>>(db).await.unwrap();
            assert_eq!(calls.get(), 1);
            let mut db = worker
                .offer()
                .await
                .unwrap()
                .recv::<Msg<10, Endpoint<'_, 3>>>()
                .await
                .unwrap();
            db.send::<Msg<11, ()>>(&()).await.unwrap();
            service.recv::<Msg<11, ()>>().await.unwrap();
            service.send::<Msg<12, ()>>(&()).await.unwrap();
            db.recv::<Msg<12, ()>>().await.unwrap();
            worker.send::<Msg<13, Endpoint<'_, 3>>>(db).await.unwrap();
            let _db = owner.recv::<Msg<13, Endpoint<'_, 3>>>().await.unwrap();
        });
}

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;
const SIDE_ROLE: u8 = 3;
const OBSERVER_ROLE: u8 = 4;

const ROUTE_PAYLOAD: u8 = 173;
const ROUTE_OTHER: u8 = 174;
const SIDE_REQ: u8 = 175;
const SIDE_RET: u8 = 176;
const JOIN: u8 = 177;
const PAR_A: u8 = 201;
const PAR_B: u8 = 202;
const PAR_D: u8 = 203;
const PAR_E: u8 = 204;
const PAR_POST: u8 = 205;
const ROUTE_PAR_A: u8 = 221;
const ROUTE_PAR_B: u8 = 222;
const ROUTE_PAR_C: u8 = 223;
const ROUTE_PAR_D: u8 = 224;
const ROUTE_PAR_R: u8 = 225;
const ROUTE_PAR_POST: u8 = 226;
const ROUTE_PAR_RIGHT_SIDE: u8 = 233;
const ROUTE_PAR_RIGHT_OBSERVER: u8 = 234;
const DEAD_RIGHT_A: u8 = 227;
const DEAD_RIGHT_B: u8 = 228;
const DEAD_RIGHT_C: u8 = 229;
const DEAD_RIGHT_E: u8 = 230;
const DEAD_RIGHT_D: u8 = 231;
const DEAD_RIGHT_POST: u8 = 232;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let routed = g::route(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAYLOAD, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_OTHER, u8>>(),
    );
    let side = g::seq(
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<SIDE_REQ, u8>>(),
        g::send::<SIDE_ROLE, LOCAL_ROLE, Msg<SIDE_RET, u8>>(),
    );
    project(&g::seq(
        g::par(routed, side),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<JOIN, u8>>(),
    ))
}

fn nested_parallel_join_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<PAR_A, u8>>(),
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<PAR_B, u8>>(),
    );
    let left = g::seq(inner, g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<PAR_D, u8>>());
    let right = g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<PAR_E, u8>>();
    project(&g::seq(
        g::par(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<PAR_POST, u8>>(),
    ))
}

fn route_left_nested_parallel_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let nested_join = g::par(
        g::par(
            g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_A, u8>>(),
            g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<ROUTE_PAR_B, u8>>(),
        ),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_C, u8>>(),
    );
    let left = g::seq(
        nested_join,
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_D, u8>>(),
    );
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_R, u8>>(),
        g::par(
            g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<ROUTE_PAR_RIGHT_SIDE, u8>>(),
            g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_RIGHT_OBSERVER, u8>>(),
        ),
    );
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_POST, u8>>(),
    ))
}

fn route_right_parallel_dead_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn parallel_route_right_parallel_dead_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    let routed = g::route(left, right);
    let sibling = g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_E, u8>>();
    project(&g::seq(
        g::par(routed, sibling),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn outer_left_kills_nested_right_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let inner_left = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    let inner_right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_D, u8>>();
    let right = g::route(inner_left, inner_right);
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn assert_join_blocked(rendered: &str) {
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "post-par join must be rejected by resident progress evidence: {rendered}"
    );
}

async fn assert_send_rejected<F>(future: F, context: &str)
where
    F: core::future::Future<Output = core::result::Result<(), hibana::EndpointError>>,
{
    let err = future.await.expect_err(context);
    assert_join_blocked(&format!("{err:?}"));
}

macro_rules! attach_three_roles {
    ($rv:expr, $sid:expr, $program:ident) => {{
        let sid = SessionId::new($sid);
        (
            $rv.enter(sid, &$program::<LOCAL_ROLE>())
                .expect("attach local role"),
            $rv.enter(sid, &$program::<WORKER_ROLE>())
                .expect("attach worker role"),
            $rv.enter(sid, &$program::<OBSERVER_ROLE>())
                .expect("attach observer role"),
        )
    }};
}

macro_rules! attach_four_roles {
    ($rv:expr, $sid:expr, $program:ident) => {{
        let sid = SessionId::new($sid);
        (
            $rv.enter(sid, &$program::<LOCAL_ROLE>())
                .expect("attach local role"),
            $rv.enter(sid, &$program::<WORKER_ROLE>())
                .expect("attach worker role"),
            $rv.enter(sid, &$program::<SIDE_ROLE>())
                .expect("attach side role"),
            $rv.enter(sid, &$program::<OBSERVER_ROLE>())
                .expect("attach observer role"),
        )
    }};
}

#[test]
fn unselected_route_arm_parallel_events_are_dead_and_not_join_obligations() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                {
                    let (mut local, mut worker, observer) =
                        attach_three_roles!(rv, 98, route_right_parallel_dead_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_send_rejected(
                        local.send::<Msg<DEAD_RIGHT_B, u8>>(&0),
                        "unselected right nested-par B must be dead",
                    )
                    .await;
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, observer) =
                        attach_three_roles!(rv, 99, route_right_parallel_dead_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_send_rejected(
                        local.send::<Msg<DEAD_RIGHT_C, u8>>(&0),
                        "unselected right nested-par C must be dead",
                    )
                    .await;
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, mut observer) =
                        attach_three_roles!(rv, 100, route_right_parallel_dead_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    local
                        .send::<Msg<DEAD_RIGHT_POST, u8>>(&2)
                        .await
                        .expect("send post route");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                            .await
                            .expect("recv post"),
                        2
                    );
                }
            });
        });
    });
}

#[test]
fn unselected_route_arm_parallel_events_do_not_block_parallel_join() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                {
                    let (mut local, mut worker, observer) =
                        attach_three_roles!(rv, 101, parallel_route_right_parallel_dead_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_send_rejected(
                        local.send::<Msg<DEAD_RIGHT_POST, u8>>(&0),
                        "outer par join must still wait for sibling E",
                    )
                    .await;
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, mut observer) =
                        attach_three_roles!(rv, 102, parallel_route_right_parallel_dead_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    local
                        .send::<Msg<DEAD_RIGHT_E, u8>>(&2)
                        .await
                        .expect("send E");
                    local
                        .send::<Msg<DEAD_RIGHT_POST, u8>>(&3)
                        .await
                        .expect("send post");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<DEAD_RIGHT_E, u8>>()
                            .await
                            .expect("recv E"),
                        2
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                            .await
                            .expect("recv post"),
                        3
                    );
                }
            });
        });
    });
}

#[test]
fn outer_left_selection_kills_nested_right_route_and_parallel_body() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                for (sid, label) in [
                    (103, DEAD_RIGHT_B),
                    (104, DEAD_RIGHT_C),
                    (105, DEAD_RIGHT_D),
                ] {
                    let (mut local, mut worker, observer) =
                        attach_three_roles!(rv, sid, outer_left_kills_nested_right_route_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    let rejected = match label {
                        DEAD_RIGHT_B => local.send::<Msg<DEAD_RIGHT_B, u8>>(&0).await,
                        DEAD_RIGHT_C => local.send::<Msg<DEAD_RIGHT_C, u8>>(&0).await,
                        DEAD_RIGHT_D => local.send::<Msg<DEAD_RIGHT_D, u8>>(&0).await,
                        _ => unreachable!(),
                    }
                    .expect_err("nested right event must be dead after outer left selection");
                    assert_join_blocked(&format!("{rejected:?}"));
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, mut observer) =
                        attach_three_roles!(rv, 106, outer_left_kills_nested_right_route_program);
                    local
                        .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                        .await
                        .expect("send A");
                    local
                        .send::<Msg<DEAD_RIGHT_POST, u8>>(&2)
                        .await
                        .expect("send post route");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch
                            .recv::<Msg<DEAD_RIGHT_A, u8>>()
                            .await
                            .expect("recv A"),
                        1
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                            .await
                            .expect("recv post"),
                        2
                    );
                }
            });
        });
    });
}

#[test]
fn route_selected_left_keeps_entire_nested_parallel_path_live() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                {
                    let (mut local, mut worker, side, observer) =
                        attach_four_roles!(rv, 107, route_left_nested_parallel_program);
                    local
                        .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                        1
                    );
                    assert_send_rejected(
                        local.send::<Msg<ROUTE_PAR_R, u8>>(&0),
                        "right arm must be unselected after A commits",
                    )
                    .await;
                    core::hint::black_box((side, observer));
                }
                {
                    let (mut local, mut worker, side, observer) =
                        attach_four_roles!(rv, 108, route_left_nested_parallel_program);
                    local
                        .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                        .await
                        .expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                        1
                    );
                    assert_send_rejected(
                        local.send::<Msg<ROUTE_PAR_D, u8>>(&0),
                        "D must wait for B and C",
                    )
                    .await;
                    core::hint::black_box((side, observer));
                }
                {
                    let (mut local, mut worker, mut side, observer) =
                        attach_four_roles!(rv, 109, route_left_nested_parallel_program);
                    local
                        .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                        .await
                        .expect("send A");
                    local
                        .send::<Msg<ROUTE_PAR_B, u8>>(&2)
                        .await
                        .expect("send B");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                        1
                    );
                    assert_eq!(
                        side.recv::<Msg<ROUTE_PAR_B, u8>>().await.expect("recv B"),
                        2
                    );
                    assert_send_rejected(
                        local.send::<Msg<ROUTE_PAR_D, u8>>(&0),
                        "D must still wait for C",
                    )
                    .await;
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, mut side, mut observer) =
                        attach_four_roles!(rv, 110, route_left_nested_parallel_program);
                    local
                        .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                        .await
                        .expect("send A");
                    local
                        .send::<Msg<ROUTE_PAR_B, u8>>(&2)
                        .await
                        .expect("send B");
                    local
                        .send::<Msg<ROUTE_PAR_C, u8>>(&3)
                        .await
                        .expect("send C");
                    local
                        .send::<Msg<ROUTE_PAR_D, u8>>(&4)
                        .await
                        .expect("send D");
                    local
                        .send::<Msg<ROUTE_PAR_POST, u8>>(&5)
                        .await
                        .expect("send Post");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                        1
                    );
                    assert_eq!(
                        side.recv::<Msg<ROUTE_PAR_B, u8>>().await.expect("recv B"),
                        2
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<ROUTE_PAR_C, u8>>()
                            .await
                            .expect("recv C"),
                        3
                    );
                    assert_eq!(
                        worker.recv::<Msg<ROUTE_PAR_D, u8>>().await.expect("recv D"),
                        4
                    );
                    assert_eq!(
                        observer
                            .recv::<Msg<ROUTE_PAR_POST, u8>>()
                            .await
                            .expect("recv Post"),
                        5
                    );
                }
            });
        });
    });
}

#[test]
fn route_inside_parallel_lane_cannot_release_join_before_sibling_lane() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                {
                    let (mut local, mut worker, side, observer) =
                        attach_four_roles!(rv, 111, program);
                    local
                        .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                        .await
                        .expect("send route payload");
                    let branch = worker.offer().await.expect("offer route payload");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                        10
                    );
                    assert_send_rejected(
                        local.send::<Msg<JOIN, u8>>(&0),
                        "join must wait for the sibling parallel lane",
                    )
                    .await;
                    core::hint::black_box((side, observer));
                }
                {
                    let (mut local, mut worker, mut side, observer) =
                        attach_four_roles!(rv, 112, program);
                    local
                        .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                        .await
                        .expect("send route payload");
                    local
                        .send::<Msg<SIDE_REQ, u8>>(&20)
                        .await
                        .expect("send side");
                    let branch = worker.offer().await.expect("offer route payload");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                        10
                    );
                    assert_eq!(
                        side.recv::<Msg<SIDE_REQ, u8>>().await.expect("recv side"),
                        20
                    );
                    assert_send_rejected(
                        local.send::<Msg<JOIN, u8>>(&0),
                        "join must wait for the sibling lane response",
                    )
                    .await;
                    core::hint::black_box(observer);
                }
                {
                    let (mut local, mut worker, mut side, mut observer) =
                        attach_four_roles!(rv, 113, program);
                    local
                        .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                        .await
                        .expect("send route payload");
                    local
                        .send::<Msg<SIDE_REQ, u8>>(&20)
                        .await
                        .expect("send side");
                    let branch = worker.offer().await.expect("offer route payload");
                    assert_eq!(
                        branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                        10
                    );
                    assert_eq!(
                        side.recv::<Msg<SIDE_REQ, u8>>().await.expect("recv side"),
                        20
                    );
                    side.send::<Msg<SIDE_RET, u8>>(&30)
                        .await
                        .expect("send response");
                    assert_eq!(
                        local
                            .recv::<Msg<SIDE_RET, u8>>()
                            .await
                            .expect("recv response"),
                        30
                    );
                    local.send::<Msg<JOIN, u8>>(&40).await.expect("send join");
                    assert_eq!(
                        observer.recv::<Msg<JOIN, u8>>().await.expect("recv join"),
                        40
                    );
                }
            });
        });
    });
}

#[test]
fn nested_parallel_join_requires_every_dependency_before_post() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            futures::executor::block_on(async {
                {
                    let (mut local, worker, side, mut observer) =
                        attach_four_roles!(rv, 114, nested_parallel_join_program);
                    local.send::<Msg<PAR_E, u8>>(&4).await.expect("send E");
                    assert_eq!(observer.recv::<Msg<PAR_E, u8>>().await.expect("recv E"), 4);
                    assert_send_rejected(
                        local.send::<Msg<PAR_POST, u8>>(&0),
                        "Post must still wait for the left parallel branch",
                    )
                    .await;
                    core::hint::black_box((worker, side));
                }
                {
                    let (mut local, mut worker, side, observer) =
                        attach_four_roles!(rv, 115, nested_parallel_join_program);
                    local.send::<Msg<PAR_A, u8>>(&1).await.expect("send A");
                    assert_eq!(worker.recv::<Msg<PAR_A, u8>>().await.expect("recv A"), 1);
                    assert_send_rejected(local.send::<Msg<PAR_D, u8>>(&0), "D must wait for B")
                        .await;
                    core::hint::black_box((side, observer));
                }
            });
        });
    });
}

#[test]
fn nested_parallel_join_commits_post_after_sibling_first_completion() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(94);

            let mut local = rv
                .enter(sid, &nested_parallel_join_program::<LOCAL_ROLE>())
                .expect("attach local role");
            let mut worker = rv
                .enter(sid, &nested_parallel_join_program::<WORKER_ROLE>())
                .expect("attach worker role");
            let mut side = rv
                .enter(sid, &nested_parallel_join_program::<SIDE_ROLE>())
                .expect("attach side role");
            let mut observer = rv
                .enter(sid, &nested_parallel_join_program::<OBSERVER_ROLE>())
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .send::<Msg<PAR_E, u8>>(&4)
                    .await
                    .expect("send E before nested left branch completes");
                local.send::<Msg<PAR_A, u8>>(&1).await.expect("send A");
                local.send::<Msg<PAR_B, u8>>(&2).await.expect("send B");
                local.send::<Msg<PAR_D, u8>>(&3).await.expect("send D");
                local
                    .send::<Msg<PAR_POST, u8>>(&5)
                    .await
                    .expect("send Post");

                assert_eq!(worker.recv::<Msg<PAR_A, u8>>().await.expect("recv A"), 1);
                assert_eq!(side.recv::<Msg<PAR_B, u8>>().await.expect("recv B"), 2);
                assert_eq!(worker.recv::<Msg<PAR_D, u8>>().await.expect("recv D"), 3);
                assert_eq!(observer.recv::<Msg<PAR_E, u8>>().await.expect("recv E"), 4);
                assert_eq!(
                    observer
                        .recv::<Msg<PAR_POST, u8>>()
                        .await
                        .expect("recv Post"),
                    5
                );
            });
        });
    });
}

#[test]
fn independent_rolled_admission_send_is_enabled_while_abi_input_is_quiet() {
    let bootstrap = g::seq(
        g::seq(g::send::<2, 0, Msg<1, ()>>(), g::send::<0, 2, Msg<2, ()>>()),
        g::seq(g::send::<2, 1, Msg<3, ()>>(), g::send::<1, 2, Msg<4, ()>>()),
    );
    let abi = g::route(
        g::seq(g::send::<1, 0, Msg<5, ()>>(), g::send::<0, 1, Msg<6, ()>>()),
        g::seq(g::send::<1, 0, Msg<7, ()>>(), g::send::<0, 1, Msg<8, ()>>()),
    )
    .roll();
    let admission = g::seq(
        g::send::<0, 2, Msg<9, ()>>(),
        g::route(
            g::send::<2, 0, Msg<10, ()>>(),
            g::send::<2, 0, Msg<11, ()>>(),
        ),
    )
    .roll();
    let body = g::seq(
        bootstrap,
        g::par(
            g::par(
                abi,
                g::par(
                    g::send::<0, 2, Msg<12, ()>>(),
                    g::send::<0, 2, Msg<13, ()>>(),
                ),
            ),
            admission,
        ),
    );
    let app_program: RoleProgram<0> = project(&body);
    let abi_program: RoleProgram<1> = project(&body);
    let owner_program: RoleProgram<2> = project(&body);
    let mut slab = [0u8; 16384];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let transport = TestTransport::new();
    let rendezvous = kit.rendezvous(&mut slab, transport.clone()).unwrap();
    let sid = SessionId::new(123);
    let mut app = rendezvous.enter(sid, &app_program).unwrap();
    let mut abi = rendezvous.enter(sid, &abi_program).unwrap();
    let mut owner = rendezvous.enter(sid, &owner_program).unwrap();
    futures::executor::block_on(async {
        owner.send::<Msg<1, ()>>(&()).await.unwrap();
        app.recv::<Msg<1, ()>>().await.unwrap();
        app.send::<Msg<2, ()>>(&()).await.unwrap();
        owner.recv::<Msg<2, ()>>().await.unwrap();
        owner.send::<Msg<3, ()>>(&()).await.unwrap();
        abi.recv::<Msg<3, ()>>().await.unwrap();
        abi.send::<Msg<4, ()>>(&()).await.unwrap();
        owner.recv::<Msg<4, ()>>().await.unwrap();
        app.send::<Msg<9, ()>>(&()).await.unwrap();
        owner.recv::<Msg<9, ()>>().await.unwrap();
        owner.send::<Msg<10, ()>>(&()).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<10, ()>>()
            .await
            .unwrap();
        // Return to the untouched ABI lane after another rolled region moved
        // the shared cursor. Its alternate request must still select a branch.
        abi.send::<Msg<7, ()>>(&()).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<7, ()>>()
            .await
            .unwrap();
        app.send::<Msg<8, ()>>(&()).await.unwrap();
        abi.recv::<Msg<8, ()>>().await.unwrap();
    });
    assert!(transport.queue_is_empty());
}
