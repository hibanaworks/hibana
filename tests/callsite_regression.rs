mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};
fn assert_endpoint_debug_boundary(error: hibana::EndpointError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "EndpointError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_attach_debug_boundary(error: hibana::runtime::AttachError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "AttachError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_resolver_debug_boundary(error: ResolverError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "ResolverError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_compact_debug(rendered: &str) {
    for forbidden in ["file", "line", "column"] {
        assert!(
            !rendered.contains(forbidden),
            "public error Debug must not carry source location {forbidden}: {rendered}"
        );
    }
}

static UNIT_RESOLVER_STATE: () = ();
const SCHEMA_ROUTE_RESOLVER: u16 = 4120;

fn reject_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn select_right_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Right)
}

fn project_pair<const MSG_ID: u8>() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = g::send::<0, 1, Msg<MSG_ID, u32>>();
    (project(&program), project(&program))
}

#[test]
fn runtime_registers_multiple_rendezvous_from_resident_resources() {
    {
        let mut slab = [0; 6 * 16 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (role0, _) = project_pair::<7>();
            let transport = TestTransport::new();
            const CHUNK: usize = 16 * 1024;
            let (chunk0, rest) = slab.split_at_mut(CHUNK);
            let (chunk1, rest) = rest.split_at_mut(CHUNK);
            let (chunk2, rest) = rest.split_at_mut(CHUNK);
            let (chunk3, rest) = rest.split_at_mut(CHUNK);
            let (chunk4, rest) = rest.split_at_mut(CHUNK);
            let (chunk5, _) = rest.split_at_mut(CHUNK);
            for (idx, chunk) in [chunk0, chunk1, chunk2, chunk3, chunk4, chunk5]
                .into_iter()
                .enumerate()
            {
                let rv = cluster
                    .rendezvous(chunk, transport.clone())
                    .expect("runtime registry must grow from each resident rendezvous resource");
                let endpoint = rv
                    .enter(SessionId::new(700 + idx as u32), &role0)
                    .expect("wide rendezvous budget must still attach endpoints");
                core::hint::black_box(&endpoint);
                drop(endpoint);
            }
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn endpoint_errors_keep_debug_boundaries() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, target_program) = project_pair::<11>();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rv");
            let sid = SessionId::new(411);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = (origin.send::<Msg<12, u32>>(&1234))
                .await
                .expect_err("wrong send label must fail");
            assert_endpoint_debug_boundary(send_error, "send");
            let poisoned = (origin.send::<Msg<11, u32>>(&1234))
                .await
                .expect_err("wrong send label must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<11, u32>>(&1234)).await.expect("send");

            let recv_result = (target.recv::<Msg<11, i32>>()).await;
            let recv_error = match recv_result {
                Ok(_) => panic!("wrong recv payload must fail"),
                Err(error) => error,
            };
            assert_endpoint_debug_boundary(recv_error, "recv");
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn payload_schema_mismatch_cannot_publish_send_or_consume_recv() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, target_program) = project_pair::<13>();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rendezvous");
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = (origin.send::<Msg<13, i32>>(&1234_i32))
                .await
                .expect_err("same-width wrong send schema must fail before publication");
            assert!(format!("{send_error:?}").contains("PayloadMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = (origin.send::<Msg<13, u32>>(&1234_u32))
                .await
                .expect_err("wrong send schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
            assert!(transport.queue_is_empty());

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<13, u32>>(&1234_u32))
                .await
                .expect("fresh session must publish the declared payload");
            assert!(!transport.queue_is_empty());

            let recv_error = (target.recv::<Msg<13, i32>>())
                .await
                .expect_err("same-width wrong recv schema must fail before frame consumption");
            assert!(format!("{recv_error:?}").contains("PayloadMismatch"));
            assert!(
                !transport.queue_is_empty(),
                "schema rejection must leave the queued frame unconsumed"
            );
            let poisoned = (target.recv::<Msg<13, u32>>())
                .await
                .expect_err("wrong recv schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn payload_schema_tracks_selected_route_when_labels_match() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::route(
                g::send::<0, 1, Msg<14, u32>>(),
                g::send::<0, 1, Msg<14, i32>>(),
            )
            .resolve::<SCHEMA_ROUTE_RESOLVER>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rendezvous");
            rv.set_resolver(
                &origin_program,
                ResolverRef::<SCHEMA_ROUTE_RESOLVER>::decision_state(
                    &UNIT_RESOLVER_STATE,
                    select_right_from_unit,
                ),
            )
            .expect("install schema route resolver");
            let sid = SessionId::new(413);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let mismatch = (origin.send::<Msg<14, u32>>(&23))
                .await
                .expect_err("selected right arm must reject the left payload schema");
            assert!(format!("{mismatch:?}").contains("PayloadMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = (origin.send::<Msg<14, i32>>(&-17))
                .await
                .expect_err("wrong selected-arm schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<14, i32>>(&-17))
                .await
                .expect("selected right arm and payload schema must agree");
            let branch = (target.offer()).await.expect("offer right arm");
            assert_eq!(branch.label(), 14);
            assert_eq!(
                (branch.recv::<Msg<14, i32>>())
                    .await
                    .expect("branch receive schema must match the selected arm"),
                -17
            );
            assert!(transport.queue_is_empty());
        };
    };
}

#[test]
fn attach_and_resolver_errors_keep_debug_boundaries() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rendezvous_result = cluster.rendezvous(&mut slab[..1], TestTransport::new());
            let rendezvous_error = match rendezvous_result {
                Ok(_) => panic!("resource-constrained rendezvous slab must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(rendezvous_error, "rendezvous");
        };
    };

    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, _target_program) = project_pair::<42>();
            let rv = cluster
                .rendezvous(&mut slab[..2048], TestTransport::new())
                .expect("rv");
            let sid = SessionId::new(441);
            let endpoint = rv
                .enter(sid, &origin_program)
                .expect("first role lease must attach");
            let enter_result = rv.enter(sid, &origin_program);
            let enter_error = match enter_result {
                Ok(_) => panic!("duplicate live session-role lease must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(enter_error, "enter");
            drop(endpoint);

            let resolver =
                ResolverRef::<77>::decision_state(&UNIT_RESOLVER_STATE, reject_from_unit);
            let set_resolver_result = rv.set_resolver(&origin_program, resolver);
            let set_resolver_error = match set_resolver_result {
                Ok(_) => panic!("resolver without projected site must fail"),
                Err(error) => error,
            };
            assert_resolver_debug_boundary(set_resolver_error, "set_resolver");
        };
    };

    let reject_error = ResolverError::reject();
    assert_resolver_debug_boundary(reject_error, "reject");
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_sender_drop_preserves_queued_peer_progress() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<21>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(921);
            let mut sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            (sender.send::<Msg<21, u32>>(&7)).await.unwrap();
            drop(sender);
            assert_eq!((receiver.recv::<Msg<21, u32>>()).await.unwrap(), 7);
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dropping_unexecuted_endpoint_faults_the_session() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<22>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(922);
            let sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            drop(sender);
            let error = (receiver.recv::<Msg<22, u32>>()).await.unwrap_err();
            assert_endpoint_debug_boundary(error, "recv");
            assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn drop_preserves_a_committed_protocol_fault() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<23>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(923);
            let mut sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            assert!((sender.send::<Msg<24, u32>>(&7)).await.is_err());
            drop(sender);
            let error = (receiver.recv::<Msg<23, u32>>()).await.unwrap_err();
            assert!(format!("{error:?}").contains("SessionFault(ProtocolViolation)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_parallel_drop_accepts_both_committed_arm_orders() {
    for right_first in [false, true] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let global = g::par(
                    g::send::<0, 1, Msg<30, u32>>(),
                    g::send::<0, 2, Msg<31, u32>>(),
                );
                let sender_program = project::<0>(&global);
                let left_program = project::<1>(&global);
                let right_program = project::<2>(&global);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(930);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut left = rv.enter(sid, &left_program).unwrap();
                let mut right = rv.enter(sid, &right_program).unwrap();
                (async {
                    if right_first {
                        sender.send::<Msg<31, u32>>(&31).await.unwrap();
                        sender.send::<Msg<30, u32>>(&30).await.unwrap();
                    } else {
                        sender.send::<Msg<30, u32>>(&30).await.unwrap();
                        sender.send::<Msg<31, u32>>(&31).await.unwrap();
                    }
                    // Successful peer progress after Drop proves the completed
                    // sender did not poison either parallel arm.
                    drop(sender);
                    assert_eq!(left.recv::<Msg<30, u32>>().await.unwrap(), 30);
                    assert_eq!(right.recv::<Msg<31, u32>>().await.unwrap(), 31);
                })
                .await;
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn incomplete_parallel_drop_faults_each_remaining_arm() {
    for completed_arm in [None, Some(false), Some(true)] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let global = g::par(
                    g::send::<0, 1, Msg<32, u32>>(),
                    g::send::<0, 2, Msg<33, u32>>(),
                );
                let sender_program = project::<0>(&global);
                let left_program = project::<1>(&global);
                let right_program = project::<2>(&global);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(932);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut left = rv.enter(sid, &left_program).unwrap();
                let mut right = rv.enter(sid, &right_program).unwrap();
                (async {
                    match completed_arm {
                        Some(true) => {
                            sender.send::<Msg<33, u32>>(&33).await.unwrap();
                            assert_eq!(right.recv::<Msg<33, u32>>().await.unwrap(), 33);
                        }
                        Some(false) => {
                            sender.send::<Msg<32, u32>>(&32).await.unwrap();
                            assert_eq!(left.recv::<Msg<32, u32>>().await.unwrap(), 32);
                        }
                        None => {}
                    }
                    // In particular, the lexical last arm cannot hide its
                    // unexecuted sibling from the existing Drop/fault path.
                    drop(sender);
                    let left_error = left.recv::<Msg<32, u32>>().await.unwrap_err();
                    let right_error = right.recv::<Msg<33, u32>>().await.unwrap_err();
                    for error in [left_error, right_error] {
                        assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
                    }
                })
                .await;
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_drop_uses_committed_route_and_fresh_roll_visit() {
    for complete_visits in 0..=3 {
        for begin_next_visit in [false, true] {
            {
                let mut slab = [0; 64 * 1024];
                {
                    let mut storage = SessionKitStorage::uninit();
                    let cluster = storage.init();
                    let global = g::par(
                        g::seq(
                            g::send::<0, 1, Msg<36, ()>>(),
                            g::route(
                                g::send::<1, 0, Msg<37, u32>>(),
                                g::send::<1, 0, Msg<38, u32>>(),
                            ),
                        )
                        .roll(),
                        g::send::<0, 2, Msg<39, ()>>(),
                    );
                    let sender_program = project::<0>(&global);
                    let peer_program = project::<1>(&global);
                    let other_program = project::<2>(&global);
                    let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                    let sid = SessionId::new(936);
                    let mut sender = rv.enter(sid, &sender_program).unwrap();
                    let mut peer = rv.enter(sid, &peer_program).unwrap();
                    let mut other = rv.enter(sid, &other_program).unwrap();
                    (async {
                        sender.send::<Msg<39, ()>>(&()).await.unwrap();
                        for visit in 0..complete_visits {
                            sender.send::<Msg<36, ()>>(&()).await.unwrap();
                            peer.recv::<Msg<36, ()>>().await.unwrap();
                            if visit % 2 == 1 {
                                peer.send::<Msg<38, u32>>(&38).await.unwrap();
                                assert_eq!(sender.recv::<Msg<38, u32>>().await.unwrap(), 38);
                            } else {
                                peer.send::<Msg<37, u32>>(&37).await.unwrap();
                                assert_eq!(sender.recv::<Msg<37, u32>>().await.unwrap(), 37);
                            }
                        }
                        if begin_next_visit {
                            sender.send::<Msg<36, ()>>(&()).await.unwrap();
                            peer.recv::<Msg<36, ()>>().await.unwrap();
                        }
                        drop(sender);
                        if complete_visits == 0 || begin_next_visit {
                            let error = other.recv::<Msg<39, ()>>().await.unwrap_err();
                            assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
                        } else {
                            // Unchosen route alternatives are not outstanding;
                            // Drop must leave the queued sibling consumable.
                            other.recv::<Msg<39, ()>>().await.unwrap();
                        }
                    })
                    .await;
                };
            };
        }
    }
}

fn routefree_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::par(
        g::send::<2, 3, Msg<40, u32>>(),
        g::send::<2, 4, Msg<41, u32>>(),
    )
}

fn unrelated_route_then_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::route(
            g::send::<0, 1, Msg<42, ()>>(),
            g::send::<0, 1, Msg<43, ()>>(),
        ),
        routefree_parallel_sends(),
    )
}

fn unrelated_rolled_route_then_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::route(
            g::send::<0, 1, Msg<42, ()>>(),
            g::send::<0, 1, Msg<43, ()>>(),
        )
        .roll(),
        routefree_parallel_sends(),
    )
}

#[derive(Clone, Copy, Debug)]
enum ParallelCompletionProbe {
    Complete,
    MisplacedOffer,
    ReplaySend,
    ReplayRecv,
    IncompleteDrop,
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn routefree_parallel_progress_preserves_all_completion_and_fault_cases() {
    let direct = routefree_parallel_sends();
    let shifted = unrelated_route_then_parallel_sends();
    let rolled = unrelated_rolled_route_then_parallel_sends();
    let programs = [
        (
            "direct",
            project::<2>(&direct),
            project::<3>(&direct),
            project::<4>(&direct),
            project::<0>(&direct),
        ),
        (
            "shifted",
            project::<2>(&shifted),
            project::<3>(&shifted),
            project::<4>(&shifted),
            project::<0>(&shifted),
        ),
        (
            "rolled",
            project::<2>(&rolled),
            project::<3>(&rolled),
            project::<4>(&rolled),
            project::<0>(&rolled),
        ),
    ];
    // All previous27 cases plus the three left-first misplaced-offer cases.
    for (name, sender_program, left_program, right_program, observer_program) in &programs {
        for right_first in [false, true] {
            for probe in [
                ParallelCompletionProbe::Complete,
                ParallelCompletionProbe::MisplacedOffer,
                ParallelCompletionProbe::ReplaySend,
                ParallelCompletionProbe::ReplayRecv,
                ParallelCompletionProbe::IncompleteDrop,
            ] {
                eprintln!("source={name}, right_first={right_first}, probe={probe:?}");

                {
                    let mut slab = [0; 64 * 1024];
                    {
                        let mut storage = SessionKitStorage::uninit();
                        let cluster = storage.init();
                        let transport = TestTransport::new();
                        let rv = cluster.rendezvous(&mut slab, transport.clone()).unwrap();
                        let sid = SessionId::new(940);
                        let mut sender = rv.enter(sid, sender_program).unwrap();
                        let mut left = rv.enter(sid, left_program).unwrap();
                        let mut right = rv.enter(sid, right_program).unwrap();
                        (async {
                if right_first {
                    sender.send::<Msg<41, u32>>(&41).await.unwrap();
                    assert_eq!(right.recv::<Msg<41, u32>>().await.unwrap(), 41);
                } else {
                    sender.send::<Msg<40, u32>>(&40).await.unwrap();
                    assert_eq!(left.recv::<Msg<40, u32>>().await.unwrap(), 40);
                }
                assert!(transport.queue_is_empty());
                match probe {
                    ParallelCompletionProbe::Complete => {
                        if right_first {
                            sender.send::<Msg<40, u32>>(&40).await.unwrap();
                            assert_eq!(left.recv::<Msg<40, u32>>().await.unwrap(), 40);
                        } else {
                            sender.send::<Msg<41, u32>>(&41).await.unwrap();
                            assert_eq!(right.recv::<Msg<41, u32>>().await.unwrap(), 41);
                        }
                    }
                    ParallelCompletionProbe::IncompleteDrop => {}
                    ParallelCompletionProbe::MisplacedOffer => {
                        use core::{
                            future::Future,
                            pin::pin,
                            task::{Context, Poll},
                        };
                        let mut cx = Context::from_waker(std::task::Waker::noop());
                        // Regression: route-free offer traversal previously
                        // indexed a zero-capacity visit set here and panicked.
                        match pin!(sender.offer()).poll(&mut cx) {
                            Poll::Ready(Err(error)) => {
                                assert_eq!(
                                    format!("{error:?}"),
                                    "EndpointError { operation: \"offer\", kind: PhaseInvariant }"
                                );
                            }
                            Poll::Ready(Ok(_)) => panic!("send-only continuation cannot offer"),
                            Poll::Pending => panic!("misplaced offer must reject immediately"),
                        };
                    }
                    ParallelCompletionProbe::ReplayRecv => {
                        use core::{
                            future::Future,
                            pin::pin,
                            task::{Context, Poll},
                        };
                        let mut cx = Context::from_waker(std::task::Waker::noop());
                        let rejected = if right_first {
                            matches!(
                                pin!(right.recv::<Msg<41, u32>>()).poll(&mut cx),
                                Poll::Ready(Err(_))
                            )
                        } else {
                            matches!(
                                pin!(left.recv::<Msg<40, u32>>()).poll(&mut cx),
                                Poll::Ready(Err(_))
                            )
                        };
                        assert!(
                            rejected,
                            "completed receive replay must reject without waiting"
                        );
                        assert!(transport.queue_is_empty());
                    }
                    ParallelCompletionProbe::ReplaySend => {
                        let replay = if right_first {
                            sender.send::<Msg<41, u32>>(&141).await
                        } else {
                            sender.send::<Msg<40, u32>>(&140).await
                        };
                        assert!(replay.is_err(), "a completed parallel send cannot replay");
                        assert!(
                            transport.queue_is_empty(),
                            "replay must not publish a frame"
                        );
                    }
                }
            }).await;
                        assert!(transport.queue_is_empty());
                        drop(sender);
                        match probe {
                            ParallelCompletionProbe::Complete => {
                                // Existing session admission rejects a faulted generation.
                                // Another role can enter only if Drop left it healthy.
                                drop(rv.enter(sid, observer_program).unwrap());
                            }
                            ParallelCompletionProbe::MisplacedOffer
                            | ParallelCompletionProbe::ReplaySend
                            | ParallelCompletionProbe::ReplayRecv
                            | ParallelCompletionProbe::IncompleteDrop => {
                                let error = if right_first {
                                    (left.recv::<Msg<40, u32>>()).await.unwrap_err()
                                } else {
                                    (right.recv::<Msg<41, u32>>()).await.unwrap_err()
                                };
                                assert!(format!("{error:?}").contains("SessionFault"));
                                if matches!(probe, ParallelCompletionProbe::IncompleteDrop) {
                                    assert!(format!("{error:?}").contains("EndpointDropped"));
                                }
                            }
                        }
                    };
                };
            }
        }
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn filtered_route_shift_preserves_passive_child_and_full_commit_chain() {
    for choose_inner_right in [false, true] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                // The first route is unrelated to roles 2/3. Their retained
                // outer/inner scopes move from slots 1/2 to slots 0/1.
                let graph = g::seq(
                    g::route(
                        g::send::<0, 1, Msg<50, ()>>(),
                        g::send::<0, 1, Msg<51, ()>>(),
                    ),
                    g::route(
                        g::route(
                            g::send::<2, 3, Msg<52, u32>>(),
                            g::send::<2, 3, Msg<53, u32>>(),
                        ),
                        g::send::<2, 3, Msg<54, u32>>(),
                    ),
                );
                let sender_program = project::<2>(&graph);
                let receiver_program = project::<3>(&graph);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(1941);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                (async {
                    if choose_inner_right {
                        sender.send::<Msg<53, u32>>(&53).await.unwrap();
                        drop(sender);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 53);
                        assert_eq!(branch.recv::<Msg<53, u32>>().await.unwrap(), 53);
                    } else {
                        sender.send::<Msg<52, u32>>(&52).await.unwrap();
                        drop(sender);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 52);
                        assert_eq!(branch.recv::<Msg<52, u32>>().await.unwrap(), 52);
                    }
                })
                .await;
                drop(receiver);
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn filtered_entirely_empty_role_drops_without_visit_storage() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let graph = g::route(
                g::send::<0, 2, Msg<55, ()>>(),
                g::send::<0, 2, Msg<56, ()>>(),
            );
            let program = project::<1>(&graph);
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(1942);
            let sender_program = project::<0>(&graph);
            let receiver_program = project::<2>(&graph);
            let mut sender = rv.enter(sid, &sender_program).unwrap();
            let mut receiver = rv.enter(sid, &receiver_program).unwrap();
            let endpoint = rv.enter(sid, &program).unwrap();
            drop(endpoint);
            (async {
                sender.send::<Msg<55, ()>>(&()).await.unwrap();
                receiver.recv::<Msg<55, ()>>().await.unwrap();
            })
            .await;
        };
    };
}
