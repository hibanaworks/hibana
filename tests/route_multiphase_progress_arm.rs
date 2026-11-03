mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};
const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;

const ROUTE_FIRST: u8 = 183;
const ROUTE_SECOND: u8 = 184;
const ROUTE_OTHER: u8 = 185;
const POST_ROUTE: u8 = 186;

fn multiphase_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::seq(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_FIRST, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_SECOND, u8>>(),
    );
    let right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_OTHER, u8>>();
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<POST_ROUTE, u8>>(),
    ))
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn route_arm_future_phase_blocks_post_route_send() {
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    {
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        let local_program = multiphase_route_program::<LOCAL_ROLE>();
        let worker_program = multiphase_route_program::<WORKER_ROLE>();

        {
            {
                let sid = SessionId::new(93);
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                local
                    .send::<Msg<ROUTE_FIRST, u8>>(&10)
                    .await
                    .expect("send first route step");
                let branch = worker.offer().await.expect("offer first route step");
                assert_eq!(
                    branch
                        .recv::<Msg<ROUTE_FIRST, u8>>()
                        .await
                        .expect("recv first route step"),
                    10
                );
                let err = local
                    .send::<Msg<POST_ROUTE, u8>>(&0)
                    .await
                    .expect_err("post-route send must wait for the selected route path");
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
                    "post-route send must be rejected by selected path progress: {rendered}"
                );
            }

            {
                let sid = SessionId::new(94);
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                local
                    .send::<Msg<ROUTE_FIRST, u8>>(&10)
                    .await
                    .expect("send first route step");
                let branch = worker.offer().await.expect("offer first route step");
                assert_eq!(branch.label(), ROUTE_FIRST);
                assert_eq!(
                    branch
                        .recv::<Msg<ROUTE_FIRST, u8>>()
                        .await
                        .expect("recv first route step"),
                    10
                );
                local
                    .send::<Msg<ROUTE_SECOND, u8>>(&20)
                    .await
                    .expect("send second route step");
                assert_eq!(
                    worker
                        .recv::<Msg<ROUTE_SECOND, u8>>()
                        .await
                        .expect("recv second route step"),
                    20
                );
                local
                    .send::<Msg<POST_ROUTE, u8>>(&30)
                    .await
                    .expect("send post-route");
                assert_eq!(
                    worker
                        .recv::<Msg<POST_ROUTE, u8>>()
                        .await
                        .expect("recv post-route"),
                    30
                );
            }
        }
    }
}
