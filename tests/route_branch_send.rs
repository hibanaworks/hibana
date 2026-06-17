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
static BRANCH_SEND_STATE: () = ();
const BRANCH_SEND_SELECT_LEFT: u8 = 61;
const BRANCH_SEND_SELECT_RIGHT: u8 = 62;
const BRANCH_SEND_LEFT: u8 = 63;
const BRANCH_SEND_RIGHT: u8 = 64;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn branch_send_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::seq(
        g::send::<2, 2, Msg<BRANCH_SEND_SELECT_LEFT, ()>>(),
        g::send::<0, 1, Msg<BRANCH_SEND_LEFT, u32>>(),
    );
    let right = g::seq(
        g::send::<2, 2, Msg<BRANCH_SEND_SELECT_RIGHT, ()>>(),
        g::send::<0, 1, Msg<BRANCH_SEND_RIGHT, u32>>(),
    );
    project(&g::route(left, right).resolve::<BRANCH_SEND_RESOLVER>())
}

fn choose_left(
    _: &(),
) -> Result<hibana::runtime::resolver::DecisionArm, hibana::runtime::resolver::ResolverError> {
    Ok(DecisionArm::Left)
}

fn with_branch_send_workspace(
    run: impl FnOnce(
        &mut hibana::Endpoint<'static, 0>,
        &mut hibana::Endpoint<'static, 1>,
        &mut hibana::Endpoint<'static, 2>,
    ),
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
            let role2 = branch_send_program::<2>();
            rv.set_resolver(
                &role0,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install sender resolver");
            rv.set_resolver(
                &role1,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install receiver resolver");
            rv.set_resolver(
                &role2,
                ResolverRef::<BRANCH_SEND_RESOLVER>::decision_state(
                    &BRANCH_SEND_STATE,
                    choose_left,
                ),
            )
            .expect("install controller resolver");
            let mut sender = rv.enter(sid, &role0).expect("attach sender");
            let mut receiver = rv.enter(sid, &role1).expect("attach receiver");
            let mut controller = rv.enter(sid, &role2).expect("attach controller");
            run(&mut sender, &mut receiver, &mut controller);
        });
    });
}

#[test]
fn send_start_route_completes_without_dropping_branch_preview() {
    with_branch_send_workspace(|sender, receiver, controller| {
        futures::executor::block_on(async {
            controller
                .send::<Msg<BRANCH_SEND_SELECT_LEFT, ()>>(&())
                .await
                .expect("select left arm");
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
    with_branch_send_workspace(|sender, receiver, controller| {
        futures::executor::block_on(async {
            controller
                .send::<Msg<BRANCH_SEND_SELECT_LEFT, ()>>(&())
                .await
                .expect("select left arm");
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
    with_branch_send_workspace(|sender, _receiver, controller| {
        futures::executor::block_on(async {
            controller
                .send::<Msg<BRANCH_SEND_SELECT_LEFT, ()>>(&())
                .await
                .expect("select left arm");
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

    with_branch_send_workspace(|sender, receiver, controller| {
        futures::executor::block_on(async {
            controller
                .send::<Msg<BRANCH_SEND_SELECT_LEFT, ()>>(&())
                .await
                .expect("select left arm");
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
