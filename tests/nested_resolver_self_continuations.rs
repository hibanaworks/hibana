mod common;
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
fn with_owner(
    global: impl Projectable,
    choices: (Choice, Choice),
    check: impl FnOnce(&mut Endpoint<'_, 0>, &RendezvousKit<'_, '_, TestTransport>, &(Choice, Choice)),
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
    check(&mut endpoint, &kit, &choices);
}
fn assert_no_progress(kit: &RendezvousKit<'_, '_, TestTransport>) {
    for event in kit.tap() {
        assert_ne!(event.id(), tap::ENDPOINT_SEND);
        assert_ne!(event.id(), tap::ENDPOINT_RECV);
        assert_ne!(event.id(), tap::ROUTE_ARM_SELECTION);
        if event.id() == tap::RESOLVER_AUDIT {
            assert_eq!(
                event.causal_key() & 0xff,
                0xff,
                "only a rejection may audit before commit"
            );
        }
    }
}
#[test]
fn nested_right_local_arm_is_received_at_its_own_descriptor_and_audits_each_decision_once() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        |endpoint, kit, choices| {
            futures::executor::block_on(async {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                assert_no_progress(kit);
                branch.recv::<Msg<3, ()>>().await.unwrap();
            });
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 1));
            let mut ids = kit
                .tap()
                .filter(|event| event.id() == tap::RESOLVER_AUDIT)
                .map(|event| event.arg1() & 0xffff)
                .collect::<Vec<_>>();
            ids.sort_unstable();
            assert_eq!(ids, [101, 102]);
        },
    );
}
#[test]
fn dropped_nested_preview_restores_the_same_affine_choice_without_committing_progress() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        |endpoint, kit, choices| {
            futures::executor::block_on(async {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                drop(branch);
                assert_no_progress(kit);
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
            });
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (2, 1));
        },
    );
}
#[test]
fn inner_rejection_is_terminal_without_outer_success_or_event_progress() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        |endpoint, kit, choices| {
            futures::executor::block_on(async {
                choices.1.arm.set(None);
                let error = endpoint.offer().await.err().unwrap();
                assert!(format!("{error:?}").contains("ResolverReject"));
                assert_no_progress(kit);
                choices.0.set(DecisionArm::Left);
                let error = endpoint.offer().await.err().unwrap();
                assert!(format!("{error:?}").contains("SessionFault(ProtocolViolation)"));
                assert_no_progress(kit);
            });
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 1));
        },
    );
}
#[test]
fn a_future_nested_choice_is_resolved_after_its_sequential_predecessor() {
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
        |endpoint, _, choices| {
            futures::executor::block_on(async {
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
            });
        },
    );
}
#[test]
fn a_fresh_nested_region_resolves_its_outer_choice_before_a_descendant() {
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
        |endpoint, _, choices| {
            futures::executor::block_on(async {
                endpoint.send::<Msg<1, ()>>(&()).await.unwrap();
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 4);
                branch.recv::<Msg<4, ()>>().await.unwrap();
            });
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (1, 0));
        },
    );
}
#[test]
fn nested_roll_can_reenter_before_its_sequential_tail_is_consumed() {
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
        |endpoint, _, choices| {
            futures::executor::block_on(async {
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
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<3, ()>>()
                    .await
                    .unwrap();
                choices.0.set(DecisionArm::Right);
                endpoint
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<4, ()>>()
                    .await
                    .unwrap();
                endpoint.send::<Msg<5, ()>>(&()).await.unwrap();
            });
        },
    );
}

#[test]
fn nested_branch_send_preserves_preview_and_delivers_the_chosen_payload_to_its_peer() {
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
        |sender, kit, choices| {
            let peer: RoleProgram<1> = project(&wire());
            let mut receiver = kit.enter(SessionId::new(1), &peer).unwrap();
            futures::executor::block_on(async {
                let branch = sender.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                let value = 31;
                drop(branch.send::<Msg<3, u8>>(&value));
                assert_no_progress(kit);
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
            });
            assert_eq!((choices.0.calls.get(), choices.1.calls.get()), (3, 2));
        },
    );
}

#[test]
fn a_wrong_nested_payload_schema_cannot_publish_any_ancestor_choice() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Right),
            Choice::new(DecisionArm::Right),
        ),
        |endpoint, kit, _| {
            futures::executor::block_on(async {
                let branch = endpoint.offer().await.unwrap();
                assert_eq!(branch.label(), 3);
                assert!(branch.recv::<Msg<3, u8>>().await.is_err());
                assert_no_progress(kit);
            });
        },
    );
}
