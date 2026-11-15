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
                    .expect("send parallel human request");
                assert_eq!(human.recv::<Msg<HUMAN_REQ, u8>>().await.expect("recv"), 1);
                let before = rv.tap().checkpoint();
                let error = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must wait for the sensor lane");
                assert!(
                    error.is_unavailable(),
                    "join must be rejected before I/O: {error:?}"
                );
                assert_eq!(rv.tap().checkpoint(), before);
                local
                    .send::<Msg<SENSOR_REQ, u8>>(&2)
                    .await
                    .expect("send remaining sensor request");
                assert_eq!(
                    sensor
                        .recv::<Msg<SENSOR_REQ, u8>>()
                        .await
                        .expect("sensor request"),
                    2
                );
                human
                    .send::<Msg<HUMAN_TEXT, u8>>(&3)
                    .await
                    .expect("send actual human response");
                assert_eq!(
                    local
                        .recv::<Msg<HUMAN_TEXT, u8>>()
                        .await
                        .expect("human response"),
                    3
                );
                local
                    .send::<Msg<FD_READ_RET, u8>>(&4)
                    .await
                    .expect("join after both arms complete");
                assert_eq!(
                    controller
                        .recv::<Msg<FD_READ_RET, u8>>()
                        .await
                        .expect("actual join result"),
                    4
                );
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
                let before = rv.tap().checkpoint();
                let error = local
                    .send::<Msg<FD_READ_RET, u8>>(&0)
                    .await
                    .expect_err("join must wait for the human response");
                assert!(
                    error.is_unavailable(),
                    "join must be rejected before I/O: {error:?}"
                );
                assert_eq!(rv.tap().checkpoint(), before);
                human
                    .send::<Msg<HUMAN_TEXT, u8>>(&3)
                    .await
                    .expect("complete pending human work");
                assert_eq!(
                    local
                        .recv::<Msg<HUMAN_TEXT, u8>>()
                        .await
                        .expect("human result"),
                    3
                );
                local
                    .send::<Msg<FD_READ_RET, u8>>(&4)
                    .await
                    .expect("complete join after rejected early send");
                assert_eq!(
                    controller
                        .recv::<Msg<FD_READ_RET, u8>>()
                        .await
                        .expect("join result"),
                    4
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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_retry_preview_does_not_hide_a_forward_resolver_parallel_group()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type Request = g::Msg<31, u32>;
    type Rejected = g::Msg<32, ()>;
    type Accepted = g::Msg<33, ()>;
    type First = g::Msg<34, u32>;
    type Second = g::Msg<35, u32>;
    type OtherFirst = g::Msg<36, u32>;
    type OtherSecond = g::Msg<37, u32>;
    type Done = g::Msg<38, ()>;
    let request = g::send::<1, 0, Request>();
    let approved = g::seq(
        g::send::<0, 2, Accepted>(),
        g::seq(
            g::route(
                g::par(g::send::<0, 2, First>(), g::send::<0, 3, Second>()),
                g::par(
                    g::send::<0, 2, OtherFirst>(),
                    g::send::<0, 3, OtherSecond>(),
                ),
            )
            .resolve::<193>(),
            g::send::<0, 1, Done>(),
        ),
    );
    let global = g::seq(
        request,
        g::route(
            approved,
            g::seq(
                g::seq(g::send::<0, 1, Rejected>(), request).roll(),
                approved,
            ),
        )
        .resolve::<192>(),
    );
    let controller_program = project::<0>(&global);
    let client_program = project::<1>(&global);
    let first_program = project::<2>(&global);
    let second_program = project::<3>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    rv.set_resolver(
        &controller_program,
        ResolverRef::<192>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    rv.set_resolver(
        &controller_program,
        ResolverRef::<193>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let session = SessionId::new(1799);
    let mut controller = rv.enter(session, &controller_program).unwrap();
    let mut client = rv.enter(session, &client_program).unwrap();
    let mut first = rv.enter(session, &first_program).unwrap();
    let mut second = rv.enter(session, &second_program).unwrap();
    client.send::<Request>(&0).await?;
    controller.recv::<Request>().await?;
    let preview = controller.offer().await?;
    assert_eq!(preview.label(), 32);
    drop(preview);
    controller.send::<Rejected>(&()).await?;
    client.recv::<Rejected>().await?;
    client.send::<Request>(&1).await?;
    controller.recv::<Request>().await?;
    drop(controller.offer().await?);
    controller.send::<Accepted>(&()).await?;
    first.recv::<Accepted>().await?;
    let preview = controller.offer().await?;
    assert!(
        matches!(preview.label(), 34 | 35),
        "stale retry preview: {}",
        preview.label()
    );
    drop(preview);
    async {
        let [mut a, mut b] = controller.par()?;
        async {
            a.send::<First>(&11).await?;
        };
        async {
            b.send::<Second>(&22).await?;
        };
        Ok(())
    }
    .await?;
    assert_eq!(first.recv::<First>().await?, 11);
    assert_eq!(second.recv::<Second>().await?, 22);
    controller.send::<Done>(&()).await?;
    client.recv::<Done>().await?;
    assert!(network.queue_is_empty());
    Ok(())
}
