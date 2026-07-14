mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::g::Message;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

const BRANCH_SEND_RESOLVER: u16 = 42;
const FULL_ROLE_DOMAIN_RESOLVER: u16 = 43;
static BRANCH_SEND_STATE: () = ();
const BRANCH_SEND_LEFT: u8 = 63;
const BRANCH_SEND_RIGHT: u8 = 64;
const HIGH_CONTROLLER: u8 = 254;
const HIGH_PEER: u8 = 255;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn branch_send_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<BRANCH_SEND_LEFT, u32>>();
    let right = g::send::<0, 1, Msg<BRANCH_SEND_RIGHT, u32>>();
    project(&g::route(left, right).resolve::<BRANCH_SEND_RESOLVER>())
}

fn rolled_branch_send_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<BRANCH_SEND_LEFT, u32>>();
    let right = g::send::<0, 1, Msg<BRANCH_SEND_RIGHT, u32>>();
    project(
        &g::route(left, right)
            .resolve::<BRANCH_SEND_RESOLVER>()
            .roll(),
    )
}

fn full_role_domain_rolled_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<HIGH_CONTROLLER, HIGH_PEER, Msg<BRANCH_SEND_LEFT, u32>>();
    let right = g::send::<HIGH_CONTROLLER, HIGH_PEER, Msg<BRANCH_SEND_RIGHT, u32>>();
    project(
        &g::route(left, right)
            .resolve::<FULL_ROLE_DOMAIN_RESOLVER>()
            .roll(),
    )
}

fn choose_left(
    _: &(),
) -> Result<hibana::runtime::resolver::DecisionArm, hibana::runtime::resolver::ResolverError> {
    Ok(DecisionArm::Left)
}

fn with_branch_send_workspace(
    run: impl FnOnce(&mut hibana::Endpoint<'static, 0>, &mut hibana::Endpoint<'static, 1>),
) {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(904);
            let role0 = branch_send_program::<0>();
            let role1 = branch_send_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install controller resolver");
            let mut sender = rv.enter(sid, &role0).expect("attach sender");
            let mut receiver = rv.enter(sid, &role1).expect("attach receiver");
            run(&mut sender, &mut receiver);
        });
    });
}

#[test]
fn send_start_route_completes_without_dropping_branch_preview() {
    with_branch_send_workspace(|sender, receiver| {
        futures::executor::block_on(async {
            let branch = sender.offer().await.expect("offer send arm");
            assert_eq!(
                branch.label(),
                <Msg<BRANCH_SEND_LEFT, u32> as Message>::LOGICAL_LABEL
            );
            branch
                .send::<Msg<BRANCH_SEND_LEFT, u32>>(&4444)
                .await
                .expect("branch send commits route and first send");

            let branch = receiver.offer().await.expect("offer recv arm");
            assert_eq!(
                branch
                    .recv::<Msg<BRANCH_SEND_LEFT, u32>>()
                    .await
                    .expect("recv left"),
                4444
            );
        });
    });
}

#[test]
fn dropped_branch_send_future_restores_offer_preview() {
    with_branch_send_workspace(|sender, receiver| {
        futures::executor::block_on(async {
            let first_payload = 1111;
            let branch = sender.offer().await.expect("offer send arm");
            let send = branch.send::<Msg<BRANCH_SEND_LEFT, u32>>(&first_payload);
            drop(send);

            let second_payload = 2222;
            let branch = sender.offer().await.expect("re-offer send arm");
            assert_eq!(
                branch.label(),
                <Msg<BRANCH_SEND_LEFT, u32> as Message>::LOGICAL_LABEL
            );
            branch
                .send::<Msg<BRANCH_SEND_LEFT, u32>>(&second_payload)
                .await
                .expect("branch send after dropped future");
            let branch = receiver.offer().await.expect("offer recv arm");
            assert_eq!(
                branch
                    .recv::<Msg<BRANCH_SEND_LEFT, u32>>()
                    .await
                    .expect("recv left"),
                second_payload
            );
        });
    });
}

#[test]
fn branch_first_step_operation_mismatch_is_fail_closed() {
    with_branch_send_workspace(|sender, _receiver| {
        futures::executor::block_on(async {
            let branch = sender.offer().await.expect("offer send arm");
            let err = match branch.recv::<Msg<BRANCH_SEND_LEFT, u32>>().await {
                Ok(_) => panic!("send-first branch must not accept recv"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("PhaseInvariant"));
            let err = match sender.offer().await {
                Ok(_) => panic!("wrong first-step operation must poison the generation"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"offer\""));
        });
    });

    with_branch_send_workspace(|sender, receiver| {
        futures::executor::block_on(async {
            let branch = sender.offer().await.expect("offer send arm");
            branch
                .send::<Msg<BRANCH_SEND_LEFT, u32>>(&3333)
                .await
                .expect("send selected arm");
            let branch = receiver.offer().await.expect("offer recv arm");
            let err = match branch.send::<Msg<BRANCH_SEND_LEFT, u32>>(&4444).await {
                Ok(_) => panic!("recv-first branch must not accept send"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("PhaseInvariant"));
            let err = match receiver.offer().await {
                Ok(_) => panic!("wrong first-step operation must poison the generation"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"offer\""));
        });
    });
}

#[test]
fn rolled_route_pipelines_next_decision_before_receiver_observes() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let role0 = rolled_branch_send_program::<0>();
            let role1 = rolled_branch_send_program::<1>();
            rv.set_resolver(
                &role0,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install controller resolver");
            let sid = SessionId::new(905);
            let mut sender = rv.enter(sid, &role0).expect("attach sender");
            let mut receiver = rv.enter(sid, &role1).expect("attach receiver");

            futures::executor::block_on(async {
                let branch = sender.offer().await.expect("select first left arm");
                branch
                    .send::<Msg<BRANCH_SEND_LEFT, u32>>(&1)
                    .await
                    .expect("publish first selected payload");
                let second = sender.offer().await.expect("select second left arm");
                second
                    .send::<Msg<BRANCH_SEND_LEFT, u32>>(&2)
                    .await
                    .expect("pipeline second selected payload");
                for expected in [1, 2] {
                    let branch = receiver.offer().await.expect("observe buffered left arm");
                    assert_eq!(
                        branch
                            .recv::<Msg<BRANCH_SEND_LEFT, u32>>()
                            .await
                            .expect("receive buffered selected payload"),
                        expected
                    );
                }
            });
            assert!(transport.queue_is_empty());
            drop((receiver, sender));
        });
    });
}

#[test]
fn rolled_route_does_not_wait_for_roles_absent_from_local_runtime() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let role0 = rolled_branch_send_program::<0>();
            rv.set_resolver(
                &role0,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install controller resolver");
            let sid = SessionId::new(906);
            let mut sender = rv.enter(sid, &role0).expect("attach local controller");

            futures::executor::block_on(async {
                let first = sender.offer().await.expect("select first left arm");
                first
                    .send::<Msg<BRANCH_SEND_LEFT, u32>>(&1)
                    .await
                    .expect("publish first selected payload");
                let second = sender.offer().await.expect("select second left arm");
                second
                    .send::<Msg<BRANCH_SEND_LEFT, u32>>(&2)
                    .await
                    .expect("remote roles must not retain a local route cell");
            });
            assert!(!transport.queue_is_empty());
        });
    });
}

#[test]
fn full_u8_role_domain_route_roll_runs_without_role_domain_storage() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let controller_program = full_role_domain_rolled_program::<HIGH_CONTROLLER>();
            let peer_program = full_role_domain_rolled_program::<HIGH_PEER>();
            rv.set_resolver(
                &controller_program,
                ResolverRef::<FULL_ROLE_DOMAIN_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install high-role controller resolver");
            let sid = SessionId::new(907);
            let mut controller = rv.enter(sid, &controller_program).expect("attach role 254");
            let mut peer = rv.enter(sid, &peer_program).expect("attach role 255");

            futures::executor::block_on(async {
                for expected in [254u32, 255] {
                    controller
                        .offer()
                        .await
                        .expect("select high-role left arm")
                        .send::<Msg<BRANCH_SEND_LEFT, u32>>(&expected)
                        .await
                        .expect("send from role 254");
                    assert_eq!(
                        peer.offer()
                            .await
                            .expect("observe high-role left arm")
                            .recv::<Msg<BRANCH_SEND_LEFT, u32>>()
                            .await
                            .expect("receive at role 255"),
                        expected
                    );
                }
            });
            assert!(transport.queue_is_empty());
        });
    });
}
