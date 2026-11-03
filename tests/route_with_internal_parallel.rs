mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};
const ROUTE_LEFT: u8 = 145;
const ROUTE_RIGHT: u8 = 146;
const ROUTE_RIGHT_HUMAN: u8 = 147;
const ROUTE_RIGHT_SENSOR: u8 = 148;
const CONTROLLER_ROLE: u8 = 1;
const LOCAL_ROLE: u8 = 2;
const HUMAN_ROLE: u8 = 3;
const PICO2W_SENSOR_ROLE: u8 = 4;
const FD_READ_REQ: u8 = 87;
const FD_READ_RET: u8 = 88;
const HUMAN_TEXT: u8 = 151;
const HUMAN_REQ: u8 = 153;
const SENSOR_REQ: u8 = 154;

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
        g::seq(
            g::send::<CONTROLLER_ROLE, LOCAL_ROLE, Msg<11, u8>>(),
            g::par(
                g::send::<CONTROLLER_ROLE, HUMAN_ROLE, Msg<ROUTE_RIGHT_HUMAN, ()>>(),
                g::send::<CONTROLLER_ROLE, PICO2W_SENSOR_ROLE, Msg<ROUTE_RIGHT_SENSOR, ()>>(),
            ),
        ),
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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn selected_route_arm_materializes_lanes_inside_parallel_body() {
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    {
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        let controller_program = program::<CONTROLLER_ROLE>();
        let local_program = program::<LOCAL_ROLE>();
        let human_program = program::<HUMAN_ROLE>();
        let sensor_program = program::<PICO2W_SENSOR_ROLE>();

        {
            {
                let sid = SessionId::new(91);
                let mut controller = rv
                    .enter(sid, &controller_program)
                    .expect("attach controller");
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut human = rv.enter(sid, &human_program).expect("attach human role");
                let sensor = rv.enter(sid, &sensor_program).expect("attach sensor role");
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
                assert_eq!(human.recv::<Msg<HUMAN_REQ, u8>>().await.expect("recv"), 1);
                let error = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must wait for the sensor lane");
                let rendered = format!("{error:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
                    "join must be rejected by progress evidence: {rendered}"
                );
                core::hint::black_box(sensor);
            }

            {
                let sid = SessionId::new(92);
                let mut controller = rv
                    .enter(sid, &controller_program)
                    .expect("attach controller");
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut human = rv.enter(sid, &human_program).expect("attach human role");
                let mut sensor = rv.enter(sid, &sensor_program).expect("attach sensor role");
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
                    .expect("send human");
                local
                    .send::<Msg<SENSOR_REQ, u8>>(&2)
                    .await
                    .expect("send sensor");
                assert_eq!(human.recv::<Msg<HUMAN_REQ, u8>>().await.expect("recv"), 1);
                assert_eq!(sensor.recv::<Msg<SENSOR_REQ, u8>>().await.expect("recv"), 2);
                let error = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must wait for the human response");
                let rendered = format!("{error:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
                    "join must be rejected by progress evidence: {rendered}"
                );
            }

            {
                let sid = SessionId::new(93);
                let mut controller = rv
                    .enter(sid, &controller_program)
                    .expect("attach controller");
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut human = rv.enter(sid, &human_program).expect("attach human role");
                let mut sensor = rv.enter(sid, &sensor_program).expect("attach sensor role");
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
                    .expect("send human");
                local
                    .send::<Msg<SENSOR_REQ, u8>>(&2)
                    .await
                    .expect("send sensor");
                assert_eq!(human.recv::<Msg<HUMAN_REQ, u8>>().await.expect("recv"), 1);
                assert_eq!(sensor.recv::<Msg<SENSOR_REQ, u8>>().await.expect("recv"), 2);
                human
                    .send::<Msg<HUMAN_TEXT, u8>>(&3)
                    .await
                    .expect("send human response");
                assert_eq!(local.recv::<Msg<HUMAN_TEXT, u8>>().await.expect("recv"), 3);
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
            }
        }
    }
}
