mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};
const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;

const INNER_PAYLOAD: u8 = 195;
const OUTER_LATER: u8 = 196;
const OUTER_OTHER: u8 = 197;
const INNER_OTHER: u8 = 198;
const POST_OUTER: u8 = 199;

fn nested_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<INNER_PAYLOAD, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<INNER_OTHER, u8>>(),
    );
    let outer_left = g::seq(
        inner,
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<OUTER_LATER, u8>>(),
    );
    let outer_right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<OUTER_OTHER, u8>>();
    project(&g::seq(
        g::route(outer_left, outer_right),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<POST_OUTER, u8>>(),
    ))
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn inner_route_completion_does_not_exit_outer_route_early() {
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    {
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        let local_program = nested_route_program::<LOCAL_ROLE>();
        let worker_program = nested_route_program::<WORKER_ROLE>();

        {
            {
                let sid = SessionId::new(94);
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                local
                    .send::<Msg<INNER_PAYLOAD, u8>>(&40)
                    .await
                    .expect("send inner payload");
                let branch = worker.offer().await.expect("offer inner payload");
                assert_eq!(
                    branch
                        .recv::<Msg<INNER_PAYLOAD, u8>>()
                        .await
                        .expect("decode inner payload"),
                    40
                );
                let err = local
                    .send::<Msg<POST_OUTER, u8>>(&0)
                    .await
                    .expect_err("post-route send must wait for the selected route path");
                let rendered = format!("{err:?}");
                assert!(
                    rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
                    "post-route send must be rejected by selected path progress: {rendered}"
                );
            }

            {
                let sid = SessionId::new(95);
                let mut local = rv.enter(sid, &local_program).expect("attach local role");
                let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                local
                    .send::<Msg<INNER_PAYLOAD, u8>>(&40)
                    .await
                    .expect("send inner payload");
                let branch = worker.offer().await.expect("offer inner payload");
                assert_eq!(branch.label(), INNER_PAYLOAD);
                assert_eq!(
                    branch
                        .recv::<Msg<INNER_PAYLOAD, u8>>()
                        .await
                        .expect("decode inner payload"),
                    40
                );
                local
                    .send::<Msg<OUTER_LATER, u8>>(&50)
                    .await
                    .expect("send outer later");
                assert_eq!(
                    worker
                        .recv::<Msg<OUTER_LATER, u8>>()
                        .await
                        .expect("recv outer later"),
                    50
                );
                local
                    .send::<Msg<POST_OUTER, u8>>(&60)
                    .await
                    .expect("send post outer");
                assert_eq!(
                    worker
                        .recv::<Msg<POST_OUTER, u8>>()
                        .await
                        .expect("recv post outer"),
                    60
                );
            }
        }
    }
}
