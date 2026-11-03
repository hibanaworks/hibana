mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;
use common::TestTransport;
use core::cell::Cell;
use hibana::{
    Endpoint,
    g::{self, Msg},
    runtime::{
        RendezvousKit, SessionKitStorage,
        ids::SessionId,
        program::{Projectable, RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        tap,
    },
};
struct Choice {
    arm: Cell<Option<DecisionArm>>,
    calls: Cell<usize>,
}
impl Choice {
    fn new(arm: DecisionArm) -> Self {
        Self {
            arm: Cell::new(Some(arm)),
            calls: Cell::new(0),
        }
    }
    fn decide(&self) -> Result<DecisionArm, ResolverError> {
        self.calls.set(self.calls.get() + 1);
        self.arm.get().ok_or_else(ResolverError::reject)
    }
    fn set(&self, arm: DecisionArm) {
        self.arm.set(Some(arm));
    }
}
fn nested() -> impl Projectable {
    g::route(
        g::send::<0, 0, Msg<1, ()>>(),
        g::route(g::send::<0, 0, Msg<2, ()>>(), g::send::<0, 0, Msg<3, ()>>()).resolve::<102>(),
    )
    .resolve::<101>()
    .roll()
}
async fn with_owner(
    global: impl Projectable,
    choices: (Choice, Choice),
    check: impl AsyncFnOnce(
        &mut Endpoint<'_, 0>,
        &RendezvousKit<'_, '_, TestTransport>,
        &(Choice, Choice),
    ),
) {
    let program: RoleProgram<0> = project(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &program,
        ResolverRef::<101>::decision_state(&choices.0, Choice::decide),
    )
    .unwrap();
    kit.set_resolver(
        &program,
        ResolverRef::<102>::decision_state(&choices.1, Choice::decide),
    )
    .unwrap();
    let mut endpoint = kit.enter(SessionId::new(1), &program).unwrap();
    check(&mut endpoint, &kit, &choices).await;
}
fn assert_no_progress(kit: &RendezvousKit<'_, '_, TestTransport>, expected_calls: &[(u16, u8)]) {
    let mut calls = Vec::new();
    for event in kit.tap() {
        assert_ne!(event.id(), tap::ENDPOINT_SEND);
        assert_ne!(event.id(), tap::ENDPOINT_RECV);
        assert_ne!(event.id(), tap::ENDPOINT_SESSION);
        assert_ne!(event.id(), tap::SCOPE_REENTRY);
        assert_ne!(event.id(), tap::ROUTE_ARM_SELECTION);
        if event.id() == tap::RESOLVER_AUDIT {
            calls.push((event.arg1() as u16, event.causal_key() as u8));
        }
    }
    assert_eq!(
        calls, expected_calls,
        "record actual resolver calls before progress commits"
    );
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_right_local_arm_is_received_at_its_own_descriptor_and_audits_each_decision_once() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        async |endpoint, kit, choices| {
            {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                assert_no_progress(kit, &[(101, 1), (102, 1)]);
                branch.recv::<Msg<3, ()>>().await.unwrap();
            };
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 1));
            let mut ids = kit
                .tap()
                .filter(|event| event.id() == tap::RESOLVER_AUDIT)
                .map(|event| event.arg1() & 0xffff)
                .collect::<Vec<_>>();
            ids.sort_unstable();
            assert_eq!(ids, [101, 102]);
        },
    )
    .await;
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dropped_nested_preview_restores_the_same_affine_choice_without_committing_progress() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        async |endpoint, kit, choices| {
            {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                drop(branch);
                assert_no_progress(kit, &[(101, 1), (102, 1)]);
                choices.0.set(DecisionArm::Left);
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<3, ()>>()
                    .await
                    .unwrap();
                assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 1));
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<1, ()>>()
                    .await
                    .unwrap();
            };
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (2, 1));
        },
    )
    .await;
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn inner_rejection_is_terminal_without_publishing_the_outer_choice() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        async |endpoint, kit, choices| {
            {
                choices.1.arm.set(None);
                let error = endpoint.offer().await.err().unwrap();
                assert!(format!("{error:?}").contains("ResolverReject"));
                assert_no_progress(kit, &[(101, 1), (102, 255)]);
                choices.0.set(DecisionArm::Left);
                let error = endpoint.offer().await.err().unwrap();
                assert!(format!("{error:?}").contains("SessionFault(ProtocolViolation)"));
                assert_no_progress(kit, &[(101, 1), (102, 255)]);
            };
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 1));
        },
    )
    .await;
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_future_nested_choice_is_resolved_after_its_sequential_predecessor() {
    let global = g::route(
        g::seq(
            g::send::<0, 0, Msg<1, ()>>(),
            g::route(g::send::<0, 0, Msg<2, ()>>(), g::send::<0, 0, Msg<3, ()>>()).resolve::<102>(),
        ),
        g::send::<0, 0, Msg<4, ()>>(),
    )
    .resolve::<101>()
    .roll();
    with_owner(
        global,
        (
            Choice::new(DecisionArm::Left),
            Choice::new(DecisionArm::Right),
        ),
        async |endpoint, _, choices| {
            {
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<1, ()>>()
                    .await
                    .unwrap();
                assert_eq!(choices.1.calls.get(), 0);
                choices.1.set(DecisionArm::Left);
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 2);
                branch.recv::<Msg<2, ()>>().await.unwrap();
            };
        },
    )
    .await;
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_fresh_nested_region_resolves_its_outer_choice_before_a_descendant() {
    let global = g::seq(
        g::send::<0, 0, Msg<1, ()>>(),
        g::route(
            g::route(g::send::<0, 0, Msg<2, ()>>(), g::send::<0, 0, Msg<3, ()>>()).resolve::<102>(),
            g::send::<0, 0, Msg<4, ()>>(),
        )
        .resolve::<101>()
        .roll(),
    );
    with_owner(
        global,
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Left),
        ),
        async |endpoint, _, choices| {
            {
                endpoint.send::<Msg<1, ()>>(&()).await.unwrap();
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 4);
                branch.recv::<Msg<4, ()>>().await.unwrap();
            };
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 0));
        },
    )
    .await;
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_roll_can_reenter_before_its_sequential_tail_is_consumed() {
    let global = g::route(
        g::seq(
            g::send::<0, 0, Msg<1, ()>>(),
            g::seq(
                g::route(
                    g::route(g::send::<0, 0, Msg<2, ()>>(), g::send::<0, 0, Msg<3, ()>>())
                        .resolve::<102>(),
                    g::send::<0, 0, Msg<4, ()>>(),
                )
                .resolve::<101>()
                .roll(),
                g::send::<0, 0, Msg<5, ()>>(),
            ),
        ),
        g::send::<0, 0, Msg<6, ()>>(),
    )
    .resolve::<101>()
    .roll();
    with_owner(
        global,
        (
            Choice::new(DecisionArm::Left),
            Choice::new(DecisionArm::Left),
        ),
        async |endpoint, _, choices| {
            {
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<1, ()>>()
                    .await
                    .unwrap();
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<2, ()>>()
                    .await
                    .unwrap();
                choices.1.set(DecisionArm::Right);
                let preview = endpoint.offer().await.unwrap();
                assert!(matches!(preview.label(), 3 | 5));
                drop(preview);
                endpoint.send::<Msg<3, ()>>(&()).await.unwrap();
                choices.0.set(DecisionArm::Right);
                let preview = endpoint.offer().await.unwrap();
                assert!(matches!(preview.label(), 4 | 5));
                drop(preview);
                endpoint.send::<Msg<4, ()>>(&()).await.unwrap();
                endpoint.send::<Msg<5, ()>>(&()).await.unwrap();
            };
        },
    )
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parked_wire_receive_survives_an_independent_typed_send() {
    let global = g::par(
        g::route(
            g::send::<0, 1, Msg<10, u8>>(),
            g::send::<0, 1, Msg<11, u8>>(),
        )
        .resolve::<101>(),
        g::send::<1, 2, Msg<12, u8>>(),
    );
    let sender_program: RoleProgram<0> = project(&global);
    let receiver_program: RoleProgram<1> = project(&global);
    let observer_program: RoleProgram<2> = project(&global);
    let choice = Choice::new(DecisionArm::Left);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &sender_program,
        ResolverRef::<101>::decision_state(&choice, Choice::decide),
    )
    .unwrap();
    let sid = SessionId::new(2);
    let mut sender = kit.enter(sid, &sender_program).unwrap();
    let mut receiver = kit.enter(sid, &receiver_program).unwrap();
    let mut observer = kit.enter(sid, &observer_program).unwrap();

    {
        sender.send::<Msg<10, u8>>(&71).await.unwrap();
        let preview = receiver.offer().await.unwrap();
        assert_eq!(preview.label(), 10);
        drop(preview);
        receiver.send::<Msg<12, u8>>(&93).await.unwrap();
        assert_eq!(observer.recv::<Msg<12, u8>>().await.unwrap(), 93);
        assert_eq!(receiver.recv::<Msg<10, u8>>().await.unwrap(), 71);
    };
    assert_eq!(choice.calls.get(), 1);
    assert_eq!(
        kit.tap()
            .filter(|event| event.id() == tap::ENDPOINT_SEND)
            .count(),
        2
    );
    assert_eq!(
        kit.tap()
            .filter(|event| event.id() == tap::ENDPOINT_RECV)
            .count(),
        2
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_branch_send_preserves_preview_and_delivers_the_chosen_payload_to_its_peer() {
    fn wire() -> impl Projectable {
        g::route(
            g::send::<0, 1, Msg<1, u8>>(),
            g::route(g::send::<0, 1, Msg<2, u8>>(), g::send::<0, 1, Msg<3, u8>>()).resolve::<102>(),
        )
        .resolve::<101>()
        .roll()
    }
    with_owner(
        wire(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        async |sender, kit, choices| {
            let peer: RoleProgram<1> = project(&wire());
            let mut receiver = kit.enter(SessionId::new(1), &peer).unwrap();
            {
                let branch = sender.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                let value = 31;
                drop(branch.send::<Msg<3, u8>>(&value));
                assert_no_progress(kit, &[(101, 1), (102, 1)]);
                sender
                    .offer()
                    .await
                    .unwrap()
                    .send::<Msg<3, u8>>(&value)
                    .await
                    .unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<3, u8>>()
                        .await
                        .unwrap(),
                    value
                );
                choices.1.set(DecisionArm::Left);
                sender
                    .offer()
                    .await
                    .unwrap()
                    .send::<Msg<2, u8>>(&21)
                    .await
                    .unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<2, u8>>()
                        .await
                        .unwrap(),
                    21
                );
                choices.0.set(DecisionArm::Left);
                sender
                    .offer()
                    .await
                    .unwrap()
                    .send::<Msg<1, u8>>(&11)
                    .await
                    .unwrap();
                assert_eq!(
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Msg<1, u8>>()
                        .await
                        .unwrap(),
                    11
                );
            };
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (3, 2));
        },
    )
    .await;
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_wrong_nested_payload_schema_cannot_publish_any_ancestor_choice() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        async |endpoint, kit, _| {
            {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                assert!(branch.recv::<Msg<3, u8>>().await.is_err());
                assert_no_progress(kit, &[(101, 1), (102, 1)]);
            };
        },
    )
    .await;
}
