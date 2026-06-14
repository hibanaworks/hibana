mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{Config, CounterClock, SessionKitStorage, ids::SessionId};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, CounterClock, 2>;

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
    let right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_R, u8>>();
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_POST, u8>>(),
    ))
}

fn route_right_parallel_dead_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
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
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
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
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
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
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "post-par join must be rejected by resident progress evidence: {rendered}"
    );
}

fn assert_flow_rejected<T, E: core::fmt::Debug>(result: Result<T, E>, context: &str) {
    let err = match result {
        Ok(_) => panic!("{context}"),
        Err(err) => err,
    };
    assert_join_blocked(&format!("{err:?}"));
}

#[test]
fn unselected_route_arm_parallel_events_are_dead_and_not_join_obligations() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(98);

            let mut local = rv
                .session(sid)
                .role(&route_right_parallel_dead_program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&route_right_parallel_dead_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut observer = rv
                .session(sid)
                .role(&route_right_parallel_dead_program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<DEAD_RIGHT_A, u8>>()
                    .expect("left route flow")
                    .send(&1)
                    .await
                    .expect("send left route event");

                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_B, u8>>(),
                    "unselected right nested-par B must be dead",
                );
                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_C, u8>>(),
                    "unselected right nested-par C must be dead",
                );

                local
                    .flow::<Msg<DEAD_RIGHT_POST, u8>>()
                    .expect("post route flow ignores unselected right par")
                    .send(&2)
                    .await
                    .expect("send post route");

                let branch = worker.offer().await.expect("offer left route event");
                assert_eq!(branch.label(), DEAD_RIGHT_A);
                assert_eq!(
                    branch
                        .decode::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("decode left route event"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post route"),
                    2
                );
            });
        });
    });
}

#[test]
fn unselected_route_arm_parallel_events_do_not_block_parallel_join() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(99);

            let mut local = rv
                .session(sid)
                .role(&parallel_route_right_parallel_dead_program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&parallel_route_right_parallel_dead_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut observer = rv
                .session(sid)
                .role(&parallel_route_right_parallel_dead_program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<DEAD_RIGHT_A, u8>>()
                    .expect("left route flow")
                    .send(&1)
                    .await
                    .expect("send left route event");

                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_B, u8>>(),
                    "unselected right nested-par B must be dead",
                );
                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_C, u8>>(),
                    "unselected right nested-par C must be dead",
                );
                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_POST, u8>>(),
                    "outer par join must still wait for sibling E",
                );

                local
                    .flow::<Msg<DEAD_RIGHT_E, u8>>()
                    .expect("parallel sibling E flow")
                    .send(&2)
                    .await
                    .expect("send parallel sibling E");
                local
                    .flow::<Msg<DEAD_RIGHT_POST, u8>>()
                    .expect("post flow after selected route and sibling")
                    .send(&3)
                    .await
                    .expect("send post");

                let branch = worker.offer().await.expect("offer left route event");
                assert_eq!(branch.label(), DEAD_RIGHT_A);
                assert_eq!(
                    branch
                        .decode::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("decode left route event"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_E, u8>>()
                        .await
                        .expect("recv sibling E"),
                    2
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post"),
                    3
                );
            });
        });
    });
}

#[test]
fn outer_left_selection_kills_nested_right_route_and_parallel_body() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(100);

            let mut local = rv
                .session(sid)
                .role(&outer_left_kills_nested_right_route_program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&outer_left_kills_nested_right_route_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut observer = rv
                .session(sid)
                .role(&outer_left_kills_nested_right_route_program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<DEAD_RIGHT_A, u8>>()
                    .expect("outer left route flow")
                    .send(&1)
                    .await
                    .expect("send outer left route event");

                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_B, u8>>(),
                    "inner-left nested-par B must be dead after outer left selection",
                );
                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_C, u8>>(),
                    "inner-left nested-par C must be dead after outer left selection",
                );
                assert_flow_rejected(
                    local.flow::<Msg<DEAD_RIGHT_D, u8>>(),
                    "inner-right D must be dead after outer left selection",
                );

                local
                    .flow::<Msg<DEAD_RIGHT_POST, u8>>()
                    .expect("post route flow ignores nested unselected right route")
                    .send(&2)
                    .await
                    .expect("send post route");

                let branch = worker.offer().await.expect("offer outer left route event");
                assert_eq!(branch.label(), DEAD_RIGHT_A);
                assert_eq!(
                    branch
                        .decode::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("decode outer left route event"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post route"),
                    2
                );
            });
        });
    });
}

#[test]
fn route_selected_left_keeps_entire_nested_parallel_path_live() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(97);

            let mut local = rv
                .session(sid)
                .role(&route_left_nested_parallel_program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&route_left_nested_parallel_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut side = rv
                .session(sid)
                .role(&route_left_nested_parallel_program::<SIDE_ROLE>())
                .enter()
                .expect("attach side role");
            let mut observer = rv
                .session(sid)
                .role(&route_left_nested_parallel_program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<ROUTE_PAR_A, u8>>()
                    .expect("A flow")
                    .send(&1)
                    .await
                    .expect("send A");

                let err = match local.flow::<Msg<ROUTE_PAR_R, u8>>() {
                    Ok(_) => panic!("right arm must be unselected after A commits"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                let err = match local.flow::<Msg<ROUTE_PAR_D, u8>>() {
                    Ok(_) => panic!("D must wait for B and C after A selects left"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                local
                    .flow::<Msg<ROUTE_PAR_B, u8>>()
                    .expect("B flow")
                    .send(&2)
                    .await
                    .expect("send B");
                let err = match local.flow::<Msg<ROUTE_PAR_D, u8>>() {
                    Ok(_) => panic!("D must still wait for C"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                local
                    .flow::<Msg<ROUTE_PAR_C, u8>>()
                    .expect("C flow")
                    .send(&3)
                    .await
                    .expect("send C");
                local
                    .flow::<Msg<ROUTE_PAR_D, u8>>()
                    .expect("D flow after A/B/C")
                    .send(&4)
                    .await
                    .expect("send D");
                local
                    .flow::<Msg<ROUTE_PAR_POST, u8>>()
                    .expect("Post flow after selected left path")
                    .send(&5)
                    .await
                    .expect("send Post");

                let branch = worker.offer().await.expect("offer A");
                assert_eq!(branch.label(), ROUTE_PAR_A);
                assert_eq!(
                    branch
                        .decode::<Msg<ROUTE_PAR_A, u8>>()
                        .await
                        .expect("decode A"),
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
            });
        });
    });
}

#[test]
fn route_inside_parallel_lane_cannot_release_join_before_sibling_lane() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(92);

            let mut local = rv
                .session(sid)
                .role(&program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut side = rv
                .session(sid)
                .role(&program::<SIDE_ROLE>())
                .enter()
                .expect("attach side role");
            let mut observer = rv
                .session(sid)
                .role(&program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<ROUTE_PAYLOAD, u8>>()
                    .expect("route payload flow")
                    .send(&10)
                    .await
                    .expect("send route payload");
                let err = match local.flow::<Msg<JOIN, u8>>() {
                    Ok(_) => panic!("join must wait for the sibling parallel lane"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                local
                    .flow::<Msg<SIDE_REQ, u8>>()
                    .expect("side request flow")
                    .send(&20)
                    .await
                    .expect("send side request");
                let err = match local.flow::<Msg<JOIN, u8>>() {
                    Ok(_) => panic!("join must wait for the sibling lane response"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                let branch = worker.offer().await.expect("offer route payload");
                assert_eq!(branch.label(), ROUTE_PAYLOAD);
                assert_eq!(
                    branch
                        .decode::<Msg<ROUTE_PAYLOAD, u8>>()
                        .await
                        .expect("decode route payload"),
                    10
                );
                assert_eq!(
                    side.recv::<Msg<SIDE_REQ, u8>>()
                        .await
                        .expect("recv side request"),
                    20
                );
                side.flow::<Msg<SIDE_RET, u8>>()
                    .expect("side response flow")
                    .send(&30)
                    .await
                    .expect("send side response");
                assert_eq!(
                    local
                        .recv::<Msg<SIDE_RET, u8>>()
                        .await
                        .expect("recv side response"),
                    30
                );

                local
                    .flow::<Msg<JOIN, u8>>()
                    .expect("post-par join flow")
                    .send(&40)
                    .await
                    .expect("send post-par join");
                assert_eq!(
                    observer
                        .recv::<Msg<JOIN, u8>>()
                        .await
                        .expect("recv post-par join"),
                    40
                );
            });
        });
    });
}

#[test]
fn nested_parallel_join_requires_every_dependency_before_post() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources((tap_buf, slab), CounterClock::zero());
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(95);

            let mut local = rv
                .session(sid)
                .role(&nested_parallel_join_program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&nested_parallel_join_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");
            let mut side = rv
                .session(sid)
                .role(&nested_parallel_join_program::<SIDE_ROLE>())
                .enter()
                .expect("attach side role");
            let mut observer = rv
                .session(sid)
                .role(&nested_parallel_join_program::<OBSERVER_ROLE>())
                .enter()
                .expect("attach observer role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<PAR_E, u8>>()
                    .expect("E sibling flow is initially enabled")
                    .send(&4)
                    .await
                    .expect("send E before nested left branch completes");
                let err = match local.flow::<Msg<PAR_POST, u8>>() {
                    Ok(_) => panic!("Post must still wait for the left parallel branch"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                local
                    .flow::<Msg<PAR_A, u8>>()
                    .expect("A flow")
                    .send(&1)
                    .await
                    .expect("send A");
                let err = match local.flow::<Msg<PAR_D, u8>>() {
                    Ok(_) => panic!("D must wait for both A and B"),
                    Err(err) => err,
                };
                assert_join_blocked(&format!("{err:?}"));

                local
                    .flow::<Msg<PAR_B, u8>>()
                    .expect("B flow")
                    .send(&2)
                    .await
                    .expect("send B");
                local
                    .flow::<Msg<PAR_D, u8>>()
                    .expect("D flow after A and B")
                    .send(&3)
                    .await
                    .expect("send D");
                local
                    .flow::<Msg<PAR_POST, u8>>()
                    .expect("Post flow after D and E")
                    .send(&5)
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
