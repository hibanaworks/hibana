#[path = "common/admission_observations.rs"]
mod admission_observations;
mod common;
use admission_observations::{observed_poll, record_contracts, record_observation};

#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::Message;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
};
const BRANCH_SEND_RESOLVER: u16 = 42;
const FULL_ROLE_DOMAIN_RESOLVER: u16 = 43;
static BRANCH_SEND_STATE: () = ();
const BRANCH_SEND_LEFT: u8 = 63;
const BRANCH_SEND_RIGHT: u8 = 64;
const HIGH_CONTROLLER: u8 = 254;
const HIGH_PEER: u8 = 255;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_fresh_send_can_wait_for_a_choice_that_skips_its_own_predecessor()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Context, task::Waker};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Default)]
    struct WakeFlag(AtomicBool);
    impl std::task::Wake for WakeFlag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    type Loan<'r> = Msg<1, hibana::Endpoint<'r, 2>>;
    type Work = Msg<2, ()>;
    type Skip = Msg<3, ()>;
    type Optional = Msg<4, ()>;
    type ResultValue = Msg<5, u8>;

    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(
                g::seq(g::send::<0, 1, Work>(), g::send::<2, 0, Optional>()),
                g::send::<0, 1, Skip>(),
            ),
            g::send::<2, 0, ResultValue>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let holder_program = project::<1>(&global);
    let resource_program = project::<2>(&global);
    record_contracts("open-choice", &holder_program);
    // First deliver the same suffix with the resource already borrowed. Then
    // poll the resource before the holder learns which arm the owner selects.
    for (poll_before_choice, work) in [(false, false), (true, false), (false, true)] {
        let network = TestTransport::new();
        let mut owner_slab = [0; 32 * 1024];
        let mut holder_slab = [0; 32 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut holder_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let holder_rv = holder_storage
            .init()
            .rendezvous(&mut holder_slab, network.clone())
            .unwrap();
        let session = SessionId::new(920);
        let mut owner = owner_rv.enter(session, &owner_program).unwrap();
        let mut holder = holder_rv.enter(session, &holder_program).unwrap();
        owner
            .send::<Loan>(owner_rv.enter(session, &resource_program).unwrap())
            .await?;
        let mut resource = holder.recv::<Loan>().await?;
        if work {
            owner.send::<Work>(&()).await?;
            holder.recv::<Work>().await?;
            {
                let mut blocked = pin!(resource.send::<ResultValue>(&17));
                let poll = blocked
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()));
                assert!(matches!(poll, core::task::Poll::Ready(Err(e)) if e.is_unavailable()));
                assert!(network.queue_is_empty());
            }
            resource.send::<Optional>(&()).await?;
            owner.recv::<Optional>().await?;
        }
        let wake = Arc::new(WakeFlag::default());
        let waker = Waker::from(Arc::clone(&wake));
        let mut send = pin!(resource.send::<ResultValue>(&17));
        if poll_before_choice {
            let poll = send.as_mut().poll(&mut Context::from_waker(&waker));
            record_observation(&format!("open-choice {}", observed_poll(&poll)));
            assert!(
                poll.is_pending(),
                "the skip arm can enable this send: {poll:?}"
            );
            assert!(network.queue_is_empty());
            wake.0.store(false, Ordering::Relaxed);
        }
        if !work {
            owner.send::<Skip>(&()).await?;
            holder.recv::<Skip>().await?;
        }
        if poll_before_choice {
            assert!(wake.0.load(Ordering::Relaxed));
        }
        send.await?;
        assert_eq!(owner.recv::<ResultValue>().await?, 17);
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn both_arms_need_the_held_view_even_when_their_predecessors_differ()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    type Loan<'r> = Msg<1, hibana::Endpoint<'r, 2>>;
    type Left = Msg<2, ()>;
    type Right = Msg<3, ()>;
    type LeftWork = Msg<4, ()>;
    type RightWork = Msg<5, ()>;
    type Done = Msg<6, ()>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::route(
                g::seq(g::send::<0, 1, Left>(), g::send::<2, 0, LeftWork>()),
                g::seq(g::send::<0, 1, Right>(), g::send::<2, 0, RightWork>()),
            ),
            g::send::<2, 0, Done>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let holder_program = project::<1>(&global);
    let resource_program = project::<2>(&global);
    for left in [false, true] {
        let network = TestTransport::new();
        let mut owner_slab = [0; 32 * 1024];
        let mut holder_slab = [0; 32 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut holder_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let holder_rv = holder_storage
            .init()
            .rendezvous(&mut holder_slab, network.clone())
            .unwrap();
        let session = SessionId::new(921);
        let mut owner = owner_rv.enter(session, &owner_program).unwrap();
        let mut holder = holder_rv.enter(session, &holder_program).unwrap();
        owner
            .send::<Loan>(owner_rv.enter(session, &resource_program).unwrap())
            .await?;
        let mut resource = holder.recv::<Loan>().await?;
        {
            let mut send = pin!(resource.send::<Done>(&()));
            let poll = send.as_mut().poll(&mut Context::from_waker(Waker::noop()));
            assert!(matches!(poll, Poll::Ready(Err(e)) if e.is_unavailable()));
            assert!(network.queue_is_empty());
        }
        if left {
            owner.send::<Left>(&()).await?;
            holder.recv::<Left>().await?;
            resource.send::<LeftWork>(&()).await?;
            owner.recv::<LeftWork>().await?;
        } else {
            owner.send::<Right>(&()).await?;
            holder.recv::<Right>().await?;
            resource.send::<RightWork>(&()).await?;
            owner.recv::<RightWork>().await?;
        }
        resource.send::<Done>(&()).await?;
        owner.recv::<Done>().await?;
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_held_resource_can_wait_for_a_choice_that_keeps_its_current_holder()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Waker},
    };

    type Loan<'r> = Msg<1, hibana::Endpoint<'r, 2>>;
    type Keep = Msg<2, ()>;
    type Value = Msg<3, u8>;
    let global = g::seq(
        g::route(g::send::<0, 1, Loan>(), g::send::<0, 1, Keep>()),
        g::send::<2, 0, Value>(),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<2>(&global);
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    let session = SessionId::new(922);
    let mut owner = owner_rv.enter(session, &owner_program).unwrap();
    let mut resource = owner_rv.enter(session, &resource_program).unwrap();
    let mut worker = worker_rv.enter(session, &worker_program).unwrap();

    let mut send = pin!(resource.send::<Value>(&23));
    let poll = send.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(
        poll.is_pending(),
        "the owner can keep this resource: {poll:?}"
    );
    assert!(network.queue_is_empty());
    owner.send::<Keep>(&()).await?;
    worker.recv::<Keep>().await?;
    send.await?;
    assert_eq!(owner.recv::<Value>().await?, 23);
    assert!(network.queue_is_empty());
    Ok(())
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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn send_start_route_completes_without_dropping_branch_preview() {
    {
        {
            let mut slab = [0; 64 * 1024];
            let transport = TestTransport::new();
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let rv = cluster
                    .rendezvous(&mut slab, transport)
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

                (async {
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
                })
                .await;
            };
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dropped_branch_send_future_restores_offer_preview() {
    {
        {
            let mut slab = [0; 64 * 1024];
            let transport = TestTransport::new();
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let rv = cluster
                    .rendezvous(&mut slab, transport)
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

                (async {
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
                })
                .await;
            };
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn branch_first_step_operation_mismatch_is_fail_closed() {
    {
        {
            let mut slab = [0; 64 * 1024];
            let transport = TestTransport::new();
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let rv = cluster
                    .rendezvous(&mut slab, transport)
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
                let _receiver = rv.enter(sid, &role1).expect("attach receiver");

                (async {
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
                })
                .await;
            };
        };
    };

    {
        {
            let mut slab = [0; 64 * 1024];
            let transport = TestTransport::new();
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let rv = cluster
                    .rendezvous(&mut slab, transport)
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

                (async {
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
                })
                .await;
            };
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_pipelines_next_decision_before_receiver_observes() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (async {
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
            })
            .await;
            assert!(transport.queue_is_empty());
            drop((receiver, sender));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn rolled_route_does_not_wait_for_roles_absent_from_local_runtime() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (async {
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
            })
            .await;
            assert!(!transport.queue_is_empty());
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn full_u8_role_domain_route_roll_runs_without_role_domain_storage() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
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

            (async {
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
            })
            .await;
            assert!(transport.queue_is_empty());
        };
    };
}
