mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};
use hibana::integration::{
    SessionKitStorage,
    ids::SessionId,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use runtime_support::with_fixture;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage =
    SessionKitStorage<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;

const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;
const SIDE_ROLE: u8 = 3;
const OBSERVER_ROLE: u8 = 4;

const ROUTE_LEFT: u8 = 171;
const ROUTE_RIGHT: u8 = 172;
const ROUTE_PAYLOAD: u8 = 173;
const ROUTE_OTHER: u8 = 174;
const SIDE_REQ: u8 = 175;
const SIDE_RET: u8 = 176;
const JOIN: u8 = 177;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let routed = g::route(
        g::seq(
            g::send::<LOCAL_ROLE, LOCAL_ROLE, Msg<ROUTE_LEFT, (), RouteDecisionKind>, 1>(),
            g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAYLOAD, u8>, 1>(),
        ),
        g::seq(
            g::send::<LOCAL_ROLE, LOCAL_ROLE, Msg<ROUTE_RIGHT, (), RouteDecisionKind>, 1>(),
            g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_OTHER, u8>, 1>(),
        ),
    );
    let side = g::seq(
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<SIDE_REQ, u8>, 2>(),
        g::send::<SIDE_ROLE, LOCAL_ROLE, Msg<SIDE_RET, u8>, 2>(),
    );
    project(&g::seq(
        g::par(routed, side),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<JOIN, u8>, 0>(),
    ))
}

fn assert_join_blocked(rendered: &str) {
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "post-par join must be rejected by resident progress evidence: {rendered}"
    );
}

#[test]
fn route_inside_parallel_lane_cannot_release_join_before_sibling_lane() {
    with_fixture(|_clock, tap_buf, slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::<DefaultLabelUniverse, _>::from_resources(
                (tap_buf, slab),
                CounterClock::new(),
            );
            let transport = TestTransport::default();
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
                    .flow::<Msg<ROUTE_LEFT, (), RouteDecisionKind>>()
                    .expect("left route decision flow")
                    .send(&())
                    .await
                    .expect("commit left route decision");
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
