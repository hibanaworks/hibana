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

fn expected_session_fault(rendered: &str) -> &'static str {
    if rendered.contains("LabelMismatch") {
        "ProtocolViolation"
    } else if rendered.contains("PhaseInvariant") {
        "ProgressInvariantViolated"
    } else {
        panic!(
            "the first invalid branch or join must report resident progress evidence: {rendered}"
        )
    }
}

fn assert_session_poisoned(rendered: &str, expected: &str) {
    assert!(
        rendered.contains("SessionFault") && rendered.contains(expected),
        "a rejected affine operation must preserve {expected}: {rendered}"
    );
}

#[test]
fn alternating_route_parallel_join_uses_only_selected_arms() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let transport = TestTransport::new();
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let local_program = alternating_route_parallel_program::<LOCAL_ROLE>();
            let worker_program = alternating_route_parallel_program::<WORKER_ROLE>();

            futures::executor::block_on(async {
                {
                    let sid = SessionId::new(96);
                    let mut local = rv.enter(sid, &local_program).expect("attach local role");
                    let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                    local.send::<Msg<ALT_A, u8>>(&1).await.expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(branch.recv::<Msg<ALT_A, u8>>().await.expect("recv A"), 1);
                    let rejected = local
                        .send::<Msg<ALT_B, u8>>(&0)
                        .await
                        .expect_err("inner right payload must be unselected");
                    let expected = expected_session_fault(&format!("{rejected:?}"));
                    let poisoned = local
                        .send::<Msg<ALT_C, u8>>(&0)
                        .await
                        .expect_err("an unselected inner arm must poison the session");
                    assert_session_poisoned(&format!("{poisoned:?}"), expected);
                }

                {
                    let sid = SessionId::new(97);
                    let mut local = rv.enter(sid, &local_program).expect("attach local role");
                    let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                    local.send::<Msg<ALT_A, u8>>(&1).await.expect("send A");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(branch.recv::<Msg<ALT_A, u8>>().await.expect("recv A"), 1);
                    let rejected = local
                        .send::<Msg<ALT_R, u8>>(&0)
                        .await
                        .expect_err("outer right payload must be unselected");
                    let expected = expected_session_fault(&format!("{rejected:?}"));
                    let poisoned = local
                        .send::<Msg<ALT_C, u8>>(&0)
                        .await
                        .expect_err("an unselected outer arm must poison the session");
                    assert_session_poisoned(&format!("{poisoned:?}"), expected);
                }

                {
                    let sid = SessionId::new(98);
                    let mut local = rv.enter(sid, &local_program).expect("attach local role");
                    let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                    local.send::<Msg<ALT_A, u8>>(&1).await.expect("send A");
                    local.send::<Msg<ALT_C, u8>>(&2).await.expect("send C");
                    local.send::<Msg<ALT_D, u8>>(&5).await.expect("send D");
                    let branch = worker.offer().await.expect("offer A");
                    assert_eq!(branch.recv::<Msg<ALT_A, u8>>().await.expect("recv A"), 1);
                    assert_eq!(worker.recv::<Msg<ALT_C, u8>>().await.expect("recv C"), 2);
                    assert_eq!(worker.recv::<Msg<ALT_D, u8>>().await.expect("recv D"), 5);
                    let rejected = local
                        .send::<Msg<ALT_POST, u8>>(&0)
                        .await
                        .expect_err("Post must wait for sibling E");
                    let expected = expected_session_fault(&format!("{rejected:?}"));
                    let poisoned = local
                        .send::<Msg<ALT_E, u8>>(&0)
                        .await
                        .expect_err("an early join attempt must poison the session");
                    assert_session_poisoned(&format!("{poisoned:?}"), expected);
                }

                {
                    let sid = SessionId::new(99);
                    let mut local = rv.enter(sid, &local_program).expect("attach local role");
                    let mut worker = rv.enter(sid, &worker_program).expect("attach worker role");
                    local.send::<Msg<ALT_A, u8>>(&1).await.expect("send A");
                    local.send::<Msg<ALT_C, u8>>(&2).await.expect("send C");
                    local.send::<Msg<ALT_D, u8>>(&5).await.expect("send D");
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
                }
            });
        });
    });
}

fn prefixed_independent_rolled_routes<const ROLE: u8>() -> RoleProgram<ROLE> {
    let first = g::seq(
        g::send::<0, 1, Msg<70, u32>>(),
        g::route(
            g::seq(
                g::send::<0, 1, Msg<71, u32>>(),
                g::send::<1, 0, Msg<72, u32>>(),
            ),
            g::send::<0, 1, Msg<73, u32>>(),
        )
        .roll(),
    );
    let sibling = g::seq(
        g::send::<0, 1, Msg<90, u32>>(),
        g::route(
            g::seq(
                g::send::<0, 1, Msg<91, u32>>(),
                g::send::<1, 0, Msg<92, u32>>(),
            ),
            g::send::<0, 1, Msg<93, u32>>(),
        )
        .roll(),
    );
    project(&g::par(first, sibling))
}

#[test]
fn completed_same_lane_prefix_does_not_hide_fresh_sibling_offer() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster.rendezvous(slab, TestTransport::new()).unwrap();
            let controller_program = prefixed_independent_rolled_routes::<0>();
            let receiver_program = prefixed_independent_rolled_routes::<1>();
            futures::executor::block_on(async {
                for (case, (use_before_retiring, retire_fresh_sibling)) in
                    [(false, false), (false, true), (true, false), (true, true)]
                        .into_iter()
                        .enumerate()
                {
                    let sid = SessionId::new(1900 + case as u32);
                    let mut controller = rv.enter(sid, &controller_program).unwrap();
                    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                    controller.send::<Msg<70, u32>>(&1).await.unwrap();
                    assert_eq!(receiver.recv::<Msg<70, u32>>().await.unwrap(), 1);
                    controller.send::<Msg<90, u32>>(&2).await.unwrap();
                    assert_eq!(receiver.recv::<Msg<90, u32>>().await.unwrap(), 2);
                    if use_before_retiring {
                        controller.send::<Msg<71, u32>>(&3).await.unwrap();
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 71);
                        assert_eq!(branch.recv::<Msg<71, u32>>().await.unwrap(), 3);
                        receiver.send::<Msg<72, u32>>(&3).await.unwrap();
                        assert_eq!(controller.recv::<Msg<72, u32>>().await.unwrap(), 3);
                    }
                    controller.send::<Msg<73, u32>>(&1).await.unwrap();
                    let branch = receiver.offer().await.unwrap();
                    assert_eq!(branch.label(), 73);
                    assert_eq!(branch.recv::<Msg<73, u32>>().await.unwrap(), 1);

                    // The ordinary cursor now points at the already consumed
                    // sibling installation, while its lane head is the fresh
                    // route. Both arms must use that still-pending route context.
                    if retire_fresh_sibling {
                        controller.send::<Msg<93, u32>>(&2).await.unwrap();
                        let branch = receiver.offer().await.expect("fresh sibling retire offer");
                        assert_eq!(branch.label(), 93);
                        assert_eq!(branch.recv::<Msg<93, u32>>().await.unwrap(), 2);
                    } else {
                        controller.send::<Msg<91, u32>>(&4).await.unwrap();
                        let branch = receiver.offer().await.expect("fresh sibling use offer");
                        assert_eq!(branch.label(), 91);
                        // Restoring an unconsumed preview remains affine.
                        drop(branch);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.recv::<Msg<91, u32>>().await.unwrap(), 4);
                        receiver.send::<Msg<92, u32>>(&4).await.unwrap();
                        assert_eq!(controller.recv::<Msg<92, u32>>().await.unwrap(), 4);
                        // Preserve reentry on an already-used sibling as well.
                        controller.send::<Msg<91, u32>>(&5).await.unwrap();
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.recv::<Msg<91, u32>>().await.unwrap(), 5);
                        receiver.send::<Msg<92, u32>>(&5).await.unwrap();
                        assert_eq!(controller.recv::<Msg<92, u32>>().await.unwrap(), 5);
                    }
                }
            });
        });
    });
}

#[test]
fn fresh_sibling_still_requires_its_installation_prefix() {
    with_runtime_workspace(|slab| {
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster.rendezvous(slab, TestTransport::new()).unwrap();
            let controller_program = prefixed_independent_rolled_routes::<0>();
            let receiver_program = prefixed_independent_rolled_routes::<1>();
            futures::executor::block_on(async {
                let sid = SessionId::new(1904);
                let mut controller = rv.enter(sid, &controller_program).unwrap();
                let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                controller.send::<Msg<70, u32>>(&1).await.unwrap();
                receiver.recv::<Msg<70, u32>>().await.unwrap();
                controller.send::<Msg<73, u32>>(&1).await.unwrap();
                let branch = receiver.offer().await.unwrap();
                branch.recv::<Msg<73, u32>>().await.unwrap();
                controller
                    .send::<Msg<91, u32>>(&2)
                    .await
                    .expect_err("missing sibling installation cannot be skipped");
            });
        });
    });
}
