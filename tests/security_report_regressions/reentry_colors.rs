//! Completed elastic scopes must not share an ambiguous inbound wire color
//! with a different currently enabled continuation. No cursor tie-break is
//! allowed: the unchanged fail-closed receiver must see a unique identity.
use super::*;

fn elastic_then_current() -> impl Projectable {
    g::seq(
        g::send::<0, 1, Msg<40, u32>>(),
        g::seq(
            g::send::<1, 0, Msg<41, u32>>(),
            g::seq(
                g::route(
                    g::send::<0, 1, Msg<52, u32>>(),
                    g::send::<0, 1, Msg<42, u32>>(),
                )
                .roll(),
                g::route(
                    g::seq(
                        g::send::<1, 0, Msg<78, u32>>(),
                        g::seq(
                            g::send::<0, 1, Msg<71, u32>>(),
                            g::route(
                                g::send::<0, 1, Msg<55, u32>>(),
                                g::send::<0, 1, Msg<72, u32>>(),
                            )
                            .roll(),
                        ),
                    ),
                    g::seq(
                        g::send::<1, 0, Msg<73, u32>>(),
                        g::send::<0, 1, Msg<74, u32>>(),
                    ),
                ),
            ),
        ),
    )
}

enum Continuation {
    Current,
    OldThenCurrent,
    RejectOldForQueuedCurrent,
}

fn exercise_current_offer(
    pending_before_ingress: bool,
    drop_preview: bool,
    continuation: Continuation,
) {
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let graph = elastic_then_current();
    let cp: RoleProgram<0> = project(&graph);
    let op: RoleProgram<1> = project(&graph);
    let mut client = rv.enter(SessionId::new(1), &cp).unwrap();
    let mut owner = rv.enter(SessionId::new(1), &op).unwrap();
    futures::executor::block_on(async {
        client.send::<Msg<40, u32>>(&40).await.unwrap();
        assert_eq!(owner.recv::<Msg<40, u32>>().await.unwrap(), 40);
        owner.send::<Msg<41, u32>>(&41).await.unwrap();
        assert_eq!(client.recv::<Msg<41, u32>>().await.unwrap(), 41);
        client.send::<Msg<42, u32>>(&42).await.unwrap();
        assert_eq!(
            owner
                .offer()
                .await
                .unwrap()
                .recv::<Msg<42, u32>>()
                .await
                .unwrap(),
            42
        );
        owner.send::<Msg<78, u32>>(&78).await.unwrap();
        assert_eq!(
            client
                .offer()
                .await
                .unwrap()
                .recv::<Msg<78, u32>>()
                .await
                .unwrap(),
            78
        );
        client.send::<Msg<71, u32>>(&71).await.unwrap();
        assert_eq!(owner.recv::<Msg<71, u32>>().await.unwrap(), 71);
    });
    if matches!(continuation, Continuation::OldThenCurrent) {
        futures::executor::block_on(async {
            client.send::<Msg<52, u32>>(&0x52aa).await.unwrap();
            let branch = owner.offer().await.unwrap();
            assert_eq!(branch.label(), 52);
            assert_eq!(branch.recv::<Msg<52, u32>>().await.unwrap(), 0x52aa);
        });
    }
    if pending_before_ingress {
        let mut pending = pin!(owner.offer());
        assert!(matches!(
            pending
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        // Drop a pending operation without inventing an application message.
    }
    futures::executor::block_on(async {
        client.send::<Msg<55, u32>>(&0x55aa).await.unwrap();
        if matches!(continuation, Continuation::RejectOldForQueuedCurrent) {
            let mut wrong = pin!(owner.recv::<Msg<52, u32>>());
            assert!(
                !matches!(
                    wrong.as_mut().poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Ok(_))
                ),
                "a queued Open must not be consumed as an older Inspect"
            );
            return;
        }
        if drop_preview {
            let preview = owner.offer().await.unwrap();
            assert_eq!(preview.label(), 55);
            drop(preview);
        }
        let branch = owner.offer().await.unwrap();
        assert_eq!(
            branch.label(),
            55,
            "old Inspect must not steal the current Open"
        );
        assert_eq!(branch.recv::<Msg<55, u32>>().await.unwrap(), 0x55aa);
    });
    if !matches!(continuation, Continuation::RejectOldForQueuedCurrent) {
        assert!(carrier.queue_is_empty());
    }
}

#[test]
fn completed_elastic_roll_and_current_route_have_unambiguous_wire_identity() {
    exercise_current_offer(false, false, Continuation::Current);
}

#[test]
fn elastic_continuation_offer_preserves_pending_and_dropped_preview() {
    exercise_current_offer(true, true, Continuation::Current);
}

#[test]
fn genuine_old_elastic_reentry_preserves_the_current_continuation() {
    exercise_current_offer(false, true, Continuation::OldThenCurrent);
}

#[test]
fn queued_current_packet_cannot_be_consumed_as_an_old_logical_type() {
    exercise_current_offer(false, false, Continuation::RejectOldForQueuedCurrent);
}
