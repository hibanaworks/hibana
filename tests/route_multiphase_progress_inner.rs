mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{Config, SessionKitStorage, ids::SessionId};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, 2>;

const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;

const INNER_PAYLOAD: u8 = 195;
const OUTER_LATER: u8 = 196;
const OUTER_OTHER: u8 = 197;
const INNER_OTHER: u8 = 198;
const POST_OUTER: u8 = 199;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

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

fn assert_flow_blocked<T, E: core::fmt::Debug>(result: Result<T, E>) {
    let err = match result {
        Ok(_) => panic!("post-route flow must wait for the selected route path"),
        Err(err) => err,
    };
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "post-route flow must be rejected by selected path progress: {rendered}"
    );
}

#[test]
fn inner_route_completion_does_not_exit_outer_route_early() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(94);
            let local_program = nested_route_program::<LOCAL_ROLE>();
            let worker_program = nested_route_program::<WORKER_ROLE>();

            let mut local = rv
                .session(sid)
                .role(&local_program)
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&worker_program)
                .enter()
                .expect("attach worker role");

            futures::executor::block_on(async {
                local
                    .flow::<Msg<INNER_PAYLOAD, u8>>()
                    .expect("inner payload flow")
                    .send(&40)
                    .await
                    .expect("send inner payload");
                assert_flow_blocked(local.flow::<Msg<POST_OUTER, u8>>());

                let branch = worker.offer().await.expect("offer inner payload");
                assert_eq!(branch.label(), INNER_PAYLOAD);
                assert_eq!(
                    branch
                        .decode::<Msg<INNER_PAYLOAD, u8>>()
                        .await
                        .expect("decode inner payload"),
                    40
                );
                local
                    .flow::<Msg<OUTER_LATER, u8>>()
                    .expect("outer later flow")
                    .send(&50)
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
                    .flow::<Msg<POST_OUTER, u8>>()
                    .expect("post outer flow")
                    .send(&60)
                    .await
                    .expect("send post outer");
                assert_eq!(
                    worker
                        .recv::<Msg<POST_OUTER, u8>>()
                        .await
                        .expect("recv post outer"),
                    60
                );
            });
        });
    });
}
