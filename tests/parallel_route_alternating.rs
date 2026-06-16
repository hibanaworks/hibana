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

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;

const ALT_D: u8 = 213;
const ALT_A: u8 = 215;
const ALT_B: u8 = 216;
const ALT_C: u8 = 217;
const ALT_R: u8 = 218;
const ALT_E: u8 = 219;
const ALT_POST: u8 = 220;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn alternating_route_parallel_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_A, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_B, u8>>(),
    );
    let outer_left = g::seq(
        g::par(inner, g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_C, u8>>()),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_D, u8>>(),
    );
    let outer_right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_R, u8>>();
    let routed = g::route(outer_left, outer_right);
    let sibling = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_E, u8>>();
    project(&g::seq(
        g::par(routed, sibling),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ALT_POST, u8>>(),
    ))
}

fn assert_join_blocked(rendered: &str) {
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("PhaseInvariant"),
        "post-par join must be rejected by resident progress evidence: {rendered}"
    );
}

#[test]
fn alternating_route_parallel_join_uses_only_selected_arms() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let config = Config::from_resources(slab);
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(96);
            let local_program = alternating_route_parallel_program::<LOCAL_ROLE>();

            let mut local = rv
                .session(sid)
                .role(&local_program)
                .enter()
                .expect("attach local role");
            let mut worker = rv
                .session(sid)
                .role(&alternating_route_parallel_program::<WORKER_ROLE>())
                .enter()
                .expect("attach worker role");

            futures::executor::block_on(async {
                local.send::<Msg<ALT_A, u8>>(&1).await.expect("send A");
                let err = local
                    .send::<Msg<ALT_B, u8>>(&0)
                    .await
                    .expect_err("inner right payload must be unselected");
                assert_join_blocked(&format!("{err:?}"));
                let err = local
                    .send::<Msg<ALT_R, u8>>(&0)
                    .await
                    .expect_err("outer right payload must be unselected");
                assert_join_blocked(&format!("{err:?}"));
                local.send::<Msg<ALT_C, u8>>(&2).await.expect("send C");
                local.send::<Msg<ALT_D, u8>>(&5).await.expect("send D");
                let err = local
                    .send::<Msg<ALT_POST, u8>>(&0)
                    .await
                    .expect_err("Post must wait for sibling E");
                assert_join_blocked(&format!("{err:?}"));

                local.send::<Msg<ALT_E, u8>>(&3).await.expect("send E");
                local
                    .send::<Msg<ALT_POST, u8>>(&4)
                    .await
                    .expect("send Post");

                let branch = worker.offer().await.expect("offer A");
                assert_eq!(branch.label(), ALT_A);
                assert_eq!(branch.recv::<Msg<ALT_A, u8>>().await.expect("recv A"), 1);
                assert_eq!(worker.recv::<Msg<ALT_C, u8>>().await.expect("recv C"), 2);
                assert_eq!(worker.recv::<Msg<ALT_D, u8>>().await.expect("recv D"), 5);
                assert_eq!(worker.recv::<Msg<ALT_E, u8>>().await.expect("recv E"), 3);
                assert_eq!(
                    worker.recv::<Msg<ALT_POST, u8>>().await.expect("recv Post"),
                    4
                );
            });
        });
    });
}
