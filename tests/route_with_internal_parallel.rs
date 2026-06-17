mod common;
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

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const ROUTE_LEFT: u8 = 145;
const ROUTE_RIGHT: u8 = 146;
const CONTROLLER_ROLE: u8 = 1;
const LOCAL_ROLE: u8 = 2;
const HUMAN_ROLE: u8 = 3;
const PICO2W_SENSOR_ROLE: u8 = 4;
const FD_READ_REQ: u8 = 87;
const FD_READ_RET: u8 = 88;
const HUMAN_TEXT: u8 = 151;
const HUMAN_REQ: u8 = 153;
const SENSOR_REQ: u8 = 154;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::seq(
        g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<ROUTE_LEFT, ()>>(),
        g::seq(
            g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<FD_READ_REQ, u8>>(),
            g::seq(
                g::par(
                    g::seq(
                        g::send::<LOCAL_ROLE, HUMAN_ROLE, Msg<HUMAN_REQ, u8>>(),
                        g::send::<HUMAN_ROLE, LOCAL_ROLE, Msg<HUMAN_TEXT, u8>>(),
                    ),
                    g::send::<LOCAL_ROLE, PICO2W_SENSOR_ROLE, Msg<SENSOR_REQ, u8>>(),
                ),
                g::send::<LOCAL_ROLE, CONTROLLER_ROLE, Msg<FD_READ_RET, u8>>(),
            ),
        ),
    );
    let right = g::seq(
        g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<ROUTE_RIGHT, ()>>(),
        g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<11, u8>>(),
    );
    let routed = g::route(left, right);
    let prefix = g::seq(
        g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<1, u8>>(),
        g::seq(
            g::send::<LOCAL_ROLE, CONTROLLER_ROLE, Msg<2, u8>>(),
            g::seq(
                g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<3, u8>>(),
                g::seq(
                    g::send::<LOCAL_ROLE, CONTROLLER_ROLE, Msg<4, u8>>(),
                    g::seq(
                        g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<5, u8>>(),
                        g::send::<LOCAL_ROLE, CONTROLLER_ROLE, Msg<6, u8>>(),
                    ),
                ),
            ),
        ),
    );
    project(&g::seq(prefix, routed))
}

#[test]
fn selected_route_arm_materializes_lanes_inside_parallel_body() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let controller_program = program::<CONTROLLER_ROLE>();
            let sid = SessionId::new(91);

            let mut controller = rv
                .session(sid)
                .role(&controller_program)
                .enter()
                .expect("attach controller");
            let mut local = rv
                .session(sid)
                .role(&program::<LOCAL_ROLE>())
                .enter()
                .expect("attach local role");
            let mut human = rv
                .session(sid)
                .role(&program::<HUMAN_ROLE>())
                .enter()
                .expect("attach human role");
            let mut sensor = rv
                .session(sid)
                .role(&program::<PICO2W_SENSOR_ROLE>())
                .enter()
                .expect("attach sensor role");

            futures::executor::block_on(async {
                controller
                    .send::<Msg<1, u8>>(&1)
                    .await
                    .expect("send prefix request 1");
                assert_eq!(local.recv::<Msg<1, u8>>().await.expect("recv prefix 1"), 1);
                local
                    .send::<Msg<2, u8>>(&2)
                    .await
                    .expect("send prefix reply 1");
                assert_eq!(
                    controller
                        .recv::<Msg<2, u8>>()
                        .await
                        .expect("recv prefix reply 1"),
                    2
                );
                controller
                    .send::<Msg<3, u8>>(&3)
                    .await
                    .expect("send prefix request 2");
                assert_eq!(local.recv::<Msg<3, u8>>().await.expect("recv prefix 2"), 3);
                local
                    .send::<Msg<4, u8>>(&4)
                    .await
                    .expect("send prefix reply 2");
                assert_eq!(
                    controller
                        .recv::<Msg<4, u8>>()
                        .await
                        .expect("recv prefix reply 2"),
                    4
                );
                controller
                    .send::<Msg<5, u8>>(&5)
                    .await
                    .expect("send prefix request 3");
                assert_eq!(local.recv::<Msg<5, u8>>().await.expect("recv prefix 3"), 5);
                local
                    .send::<Msg<6, u8>>(&6)
                    .await
                    .expect("send prefix reply 3");
                assert_eq!(
                    controller
                        .recv::<Msg<6, u8>>()
                        .await
                        .expect("recv prefix reply 3"),
                    6
                );
                controller
                    .send::<Msg<ROUTE_LEFT, ()>>(&())
                    .await
                    .expect("commit left route choice");
                let branch = local.offer().await.expect("offer selected route choice");
                assert_eq!(branch.label(), ROUTE_LEFT);
                branch
                    .recv::<Msg<ROUTE_LEFT, ()>>()
                    .await
                    .expect("decode left route choice");
                controller
                    .send::<Msg<FD_READ_REQ, u8>>(&7)
                    .await
                    .expect("send outer lane request");
                assert_eq!(
                    local
                        .recv::<Msg<FD_READ_REQ, u8>>()
                        .await
                        .expect("recv request"),
                    7
                );

                local
                    .send::<Msg<HUMAN_REQ, u8>>(&1)
                    .await
                    .expect("send parallel human request");
                let err = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must stay blocked until every parallel lane completes");
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
                    "early join must be rejected by progress evidence: {rendered}"
                );
                local
                    .send::<Msg<SENSOR_REQ, u8>>(&2)
                    .await
                    .expect("send parallel sensor request");
                let err = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must stay blocked until nested lane reply completes");
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
                    "join before nested lane reply must be rejected by progress evidence: {rendered}"
                );
                assert_eq!(
                    human
                        .recv::<Msg<HUMAN_REQ, u8>>()
                        .await
                        .expect("recv human req"),
                    1
                );
                assert_eq!(
                    sensor
                        .recv::<Msg<SENSOR_REQ, u8>>()
                        .await
                        .expect("recv sensor req"),
                    2
                );
                human
                    .send::<Msg<HUMAN_TEXT, u8>>(&3)
                    .await
                    .expect("send human response");
                assert_eq!(
                    local
                        .recv::<Msg<HUMAN_TEXT, u8>>()
                        .await
                        .expect("recv human response"),
                    3
                );
                local
                    .send::<Msg<FD_READ_RET, u8>>(&4)
                    .await
                    .expect("send join response");
                assert_eq!(
                    controller
                        .recv::<Msg<FD_READ_RET, u8>>()
                        .await
                        .expect("recv join response"),
                    4
                );
            });
        });
    });
}
