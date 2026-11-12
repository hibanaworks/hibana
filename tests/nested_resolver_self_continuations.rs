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

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_completed_converging_roll_can_be_offered_again_inside_its_chosen_parent()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Poll};
    use std::future::poll_fn;
    let global = g::route(
        g::seq(
            g::send::<0, 1, Msg<0, ()>>(),
            g::seq(g::send::<0, 2, Msg<1, ()>>(), g::send::<1, 2, Msg<2, ()>>()),
        )
        .roll(),
        g::seq(
            g::send::<0, 1, Msg<3, ()>>(),
            g::seq(
                g::send::<1, 2, Msg<4, ()>>(),
                g::seq(g::send::<2, 1, Msg<5, ()>>(), g::send::<1, 0, Msg<6, ()>>()),
            ),
        ),
    )
    .resolve::<101>();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let choice = Choice::new(DecisionArm::Left);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<101>::decision_state(&choice, Choice::decide),
    )
    .unwrap();
    let session = SessionId::new(90);
    let mut a = rv.enter(session, &p0).unwrap();
    let mut b = rv.enter(session, &p1).unwrap();
    let mut c = rv.enter(session, &p2).unwrap();
    a.send::<Msg<0, ()>>(&()).await?;
    b.recv::<Msg<0, ()>>().await?;
    a.send::<Msg<1, ()>>(&()).await?;
    c.recv::<Msg<1, ()>>().await?;
    b.send::<Msg<2, ()>>(&()).await?;
    c.recv::<Msg<2, ()>>().await?;
    assert_eq!(choice.calls.get(), 1);
    poll_fn(|cx| {
        let mut offered = pin!(a.offer());
        match offered.as_mut().poll(cx) {
            Poll::Ready(Ok(branch)) => assert_eq!(branch.label(), 0),
            Poll::Ready(Err(error)) => {
                panic!("the inner roll's next visit was rejected: {error:?}")
            }
            Poll::Pending => panic!("offer waited despite the inner roll's enabled send"),
        }
        Poll::Ready(())
    })
    .await;
    assert_eq!(choice.calls.get(), 1, "the parent is not a new decision");
    a.send::<Msg<0, ()>>(&()).await?;
    b.recv::<Msg<0, ()>>().await?;
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_child_roll_offer_survives_its_completed_parent_prefix_and_tail()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Poll};
    use std::future::poll_fn;
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<8, ()>>(),
                g::seq(
                    g::send::<0, 1, Msg<0, ()>>(),
                    g::seq(g::send::<0, 2, Msg<1, ()>>(), g::send::<1, 2, Msg<2, ()>>()),
                )
                .roll(),
            ),
            g::seq(
                g::send::<0, 1, Msg<3, ()>>(),
                g::seq(
                    g::send::<1, 2, Msg<4, ()>>(),
                    g::seq(g::send::<2, 1, Msg<5, ()>>(), g::send::<1, 0, Msg<6, ()>>()),
                ),
            ),
        )
        .resolve::<101>(),
        g::send::<0, 0, Msg<9, ()>>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let choice = Choice::new(DecisionArm::Left);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<101>::decision_state(&choice, Choice::decide),
    )
    .unwrap();
    let session = SessionId::new(91);
    let mut a = rv.enter(session, &p0).unwrap();
    let mut b = rv.enter(session, &p1).unwrap();
    let mut c = rv.enter(session, &p2).unwrap();
    a.send::<Msg<8, ()>>(&()).await?;
    b.recv::<Msg<8, ()>>().await?;
    a.send::<Msg<0, ()>>(&()).await?;
    b.recv::<Msg<0, ()>>().await?;
    a.send::<Msg<1, ()>>(&()).await?;
    c.recv::<Msg<1, ()>>().await?;
    b.send::<Msg<2, ()>>(&()).await?;
    c.recv::<Msg<2, ()>>().await?;
    a.send::<Msg<9, ()>>(&()).await?;
    choice.arm.set(None);
    poll_fn(|cx| {
        let mut offered = pin!(a.offer());
        match offered.as_mut().poll(cx) {
            Poll::Ready(Ok(branch)) => assert_eq!(branch.label(), 0),
            Poll::Ready(Err(error)) => panic!("the retained child was rejected: {error:?}"),
            Poll::Pending => panic!("the completed tail hid the inner roll's enabled send"),
        }
        Poll::Ready(())
    })
    .await;
    assert_eq!(choice.calls.get(), 1, "the parent is not a new decision");
    a.send::<Msg<0, ()>>(&()).await?;
    b.recv::<Msg<0, ()>>().await?;
    assert!(a.send::<Msg<9, ()>>(&()).await.is_err());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_outer_resolver_does_not_choose_a_nested_message_selected_branch()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Poll};
    use std::future::poll_fn;
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    )
    .resolve::<101>();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    for right in [false, true] {
        let choice = Choice::new(DecisionArm::Left);
        let mut slab = [0; 8192];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        rv.set_resolver(
            &p0,
            ResolverRef::<101>::decision_state(&choice, Choice::decide),
        )
        .unwrap();
        let session = SessionId::new(89);
        let mut sender = rv.enter(session, &p0).unwrap();
        let mut receiver = rv.enter(session, &p1).unwrap();
        poll_fn(|cx| {
            let mut offered = pin!(sender.offer());
            assert!(
                !matches!(offered.as_mut().poll(cx), Poll::Ready(Ok(_))),
                "the outer resolver does not determine either inner message"
            );
            Poll::Ready(())
        })
        .await;
        for event in rv.tap() {
            assert!(!matches!(
                event.id(),
                tap::ENDPOINT_SEND | tap::ENDPOINT_RECV | tap::ROUTE_ARM_SELECTION
            ));
        }
        if right {
            sender.send::<Msg<2, ()>>(&()).await?;
            receiver.recv::<Msg<2, ()>>().await?;
        } else {
            sender.send::<Msg<1, ()>>(&()).await?;
            receiver.recv::<Msg<1, ()>>().await?;
        }
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_inner_rework_keeps_its_containing_choice_while_a_sibling_waits()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Poll};
    use std::future::poll_fn;
    type Begin = Msg<241, ()>;
    type Approved = Msg<242, ()>;
    type Rejected = Msg<243, ()>;
    type Resubmitted = Msg<244, ()>;
    type Other = Msg<245, ()>;
    type Finish = Msg<246, ()>;
    let review = g::seq(
        g::send::<1, 0, Begin>(),
        g::route(
            g::send::<0, 1, Approved>(),
            g::seq(
                g::seq(g::send::<0, 1, Rejected>(), g::send::<1, 0, Resubmitted>()).roll(),
                g::send::<0, 1, Approved>(),
            ),
        )
        .resolve::<501>(),
    );
    let global = g::seq(
        g::par(review, g::send::<2, 0, Other>()).roll(),
        g::send::<2, 0, Finish>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let choice = Choice::new(DecisionArm::Right);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<501>::decision_state(&choice, Choice::decide),
    )
    .unwrap();
    let mut controller = rv.enter(SessionId::new(87), &p0).unwrap();
    let mut author = rv.enter(SessionId::new(87), &p1).unwrap();
    let mut other = rv.enter(SessionId::new(87), &p2).unwrap();
    author.send::<Begin>(&()).await?;
    controller.recv::<Begin>().await?;
    for _ in 0..3 {
        controller.send::<Rejected>(&()).await?;
        author.recv::<Rejected>().await?;
        author.send::<Resubmitted>(&()).await?;
        controller.recv::<Resubmitted>().await?;
    }
    choice.set(DecisionArm::Left);
    controller.send::<Approved>(&()).await?;
    let calls = choice.calls.get();
    assert_eq!(calls, 1, "the retained right arm is one decision");
    choice.arm.set(None);
    // The inner elastic body may be previewed again while Other is absent.
    // Its containing route remains right; this is not a new parent decision.
    poll_fn(|cx| {
        let mut offer = pin!(controller.offer());
        match offer.as_mut().poll(cx) {
            Poll::Ready(Ok(branch)) => {
                assert_eq!(branch.label(), 243);
                drop(branch);
            }
            Poll::Ready(Err(error)) => panic!("the retained body must be available: {error:?}"),
            Poll::Pending => panic!("the retained body must not wait for its sibling"),
        }
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        choice.calls.get(),
        calls,
        "a completed review is not a new decision"
    );
    author.recv::<Approved>().await?;
    other.send::<Other>(&()).await?;
    controller.recv::<Other>().await?;
    other.send::<Finish>(&()).await?;
    controller.recv::<Finish>().await?;
    Ok(())
}
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_later_parent_visit_previews_the_selected_rework_continuation()
-> Result<(), hibana::EndpointError> {
    use core::{future::Future, pin::pin, task::Poll};
    use std::future::poll_fn;
    type Loan = Msg<220, Endpoint<'static, 3>>;
    type Begin = Msg<221, ()>;
    type Approved = Msg<222, ()>;
    type LeftWork = Msg<223, ()>;
    type LeftValue = Msg<224, ()>;
    type LeftResult = Msg<225, ()>;
    type Rejected = Msg<226, ()>;
    type Work = Msg<227, ()>;
    type Value = Msg<228, ()>;
    type Reworked = Msg<229, ()>;
    type RejectAgain = Msg<230, ()>;
    type Ready = Msg<231, ()>;
    type Return = Msg<232, Endpoint<'static, 3>>;
    type Other = Msg<233, ()>;
    type Finish = Msg<234, ()>;
    let review = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<1, 0, Begin>(),
            g::seq(
                g::route(
                    g::seq(
                        g::send::<0, 1, Approved>(),
                        g::seq(
                            g::send::<3, 2, LeftWork>(),
                            g::seq(g::send::<2, 3, LeftValue>(), g::send::<1, 0, LeftResult>()),
                        ),
                    ),
                    g::seq(
                        g::send::<0, 1, Rejected>(),
                        g::seq(
                            g::seq(
                                g::send::<3, 2, Work>(),
                                g::seq(
                                    g::send::<2, 3, Value>(),
                                    g::seq(
                                        g::send::<1, 0, Reworked>(),
                                        g::send::<0, 1, RejectAgain>(),
                                    ),
                                ),
                            )
                            .roll(),
                            g::send::<0, 1, Ready>(),
                        ),
                    ),
                )
                .resolve::<502>(),
                g::send::<1, 0, Return>(),
            ),
        ),
    );
    let global = g::seq(
        g::par(review, g::send::<2, 0, Other>()).roll(),
        g::send::<2, 0, Finish>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    for (first, second, repeats) in [
        (DecisionArm::Left, DecisionArm::Right, 1),
        (DecisionArm::Left, DecisionArm::Right, 2),
        (DecisionArm::Right, DecisionArm::Right, 1),
        (DecisionArm::Right, DecisionArm::Right, 2),
        (DecisionArm::Right, DecisionArm::Left, 1),
        (DecisionArm::Right, DecisionArm::Left, 2),
    ] {
        let choice = Choice::new(first);
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        rv.set_resolver(
            &p0,
            ResolverRef::<502>::decision_state(&choice, Choice::decide),
        )
        .unwrap();
        let session = SessionId::new(88);
        let mut owner = rv.enter(session, &p0).unwrap();
        let mut worker = rv.enter(session, &p1).unwrap();
        let mut service = rv.enter(session, &p2).unwrap();
        let mut resource = rv.enter(session, &p3).unwrap();
        for (visit, arm) in [first, second].into_iter().enumerate() {
            choice.set(arm);
            let [mut review, mut notice] = owner.par()?;
            let [mut author] = worker.par()?;
            let [mut checks, mut other] = service.par()?;
            let (returned, (), (), (), ()) = async {
                async {
                    review.send::<Loan>(resource).await?;
                    review.recv::<Begin>().await?;
                    match arm {
                        DecisionArm::Left => {
                            review.send::<Approved>(&()).await?;
                            review.recv::<LeftResult>().await?;
                        }
                        DecisionArm::Right => {
                            review.send::<Rejected>(&()).await?;
                            for _ in 0..repeats {
                                review.recv::<Reworked>().await?;
                                assert_eq!(choice.calls.get(), visit + 1);
                                poll_fn(|cx| {
                                    let mut offered = pin!(review.offer());
                                    match offered.as_mut().poll(cx) {
                                        Poll::Ready(Ok(branch)) => assert_eq!(branch.label(), 230),
                                        Poll::Ready(Err(error)) => {
                                            panic!("selected continuation was rejected: {error:?}")
                                        }
                                        Poll::Pending => {
                                            panic!("selected continuation must not wait for input")
                                        }
                                    }
                                    Poll::Ready(())
                                })
                                .await;
                                review.send::<RejectAgain>(&()).await?;
                                assert_eq!(
                                    choice.calls.get(),
                                    visit + 1,
                                    "a continuation is not a new decision"
                                );
                            }
                            review.send::<Ready>(&()).await?;
                        }
                    }
                    review.recv::<Return>().await
                };
                async { notice.recv::<Other>().await };
                async {
                    let mut work = author.recv::<Loan>().await?;
                    author.send::<Begin>(&()).await?;
                    match arm {
                        DecisionArm::Left => {
                            author.recv::<Approved>().await?;
                            work.send::<LeftWork>(&()).await?;
                            work.recv::<LeftValue>().await?;
                            author.send::<LeftResult>(&()).await?;
                        }
                        DecisionArm::Right => {
                            author.recv::<Rejected>().await?;
                            for _ in 0..repeats {
                                work.send::<Work>(&()).await?;
                                work.recv::<Value>().await?;
                                author.send::<Reworked>(&()).await?;
                                author.recv::<RejectAgain>().await?;
                            }
                            author.recv::<Ready>().await?;
                        }
                    }
                    author.send::<Return>(work).await
                };
                async {
                    match arm {
                        DecisionArm::Left => {
                            checks.recv::<LeftWork>().await?;
                            checks.send::<LeftValue>(&()).await?;
                        }
                        DecisionArm::Right => {
                            for _ in 0..repeats {
                                checks.recv::<Work>().await?;
                                checks.send::<Value>(&()).await?;
                            }
                        }
                    }
                    Ok(())
                };
                async { other.send::<Other>(&()).await };
            }
            .await?;
            resource = returned;
        }
        service.send::<Finish>(&()).await.unwrap_or_else(|error| {
                panic!("completed visits must allow Finish: first={first:?}, second={second:?}, repeats={repeats}, {error:?}")
            });
        owner.recv::<Finish>().await?;
    }
    Ok(())
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
async fn a_different_nested_send_cannot_rechoose_a_parked_parent() {
    with_owner(
        nested(),
        (
            Choice::new(DecisionArm::Left),
            Choice::new(DecisionArm::Left),
        ),
        async |endpoint, kit, choices| {
            let preview = endpoint.offer().await.unwrap();
            assert_eq!(preview.label(), 1);
            drop(preview);
            choices.0.set(DecisionArm::Right);
            let result = endpoint.send::<Msg<2, ()>>(&()).await;
            assert!(
                result.is_err(),
                "a nested send must retain the parked parent choice"
            );
            assert_eq!(
                choices.0.calls.get(),
                1,
                "the parked parent must not be resolved again"
            );
            assert!(kit.tap().all(|event| !matches!(
                event.id(),
                tap::ENDPOINT_SEND | tap::ENDPOINT_RECV | tap::ROUTE_ARM_SELECTION
            )));
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
