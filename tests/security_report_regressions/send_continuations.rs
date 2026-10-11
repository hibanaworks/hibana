use super::*;

fn nested_reply_path() -> impl Projectable {
    g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<171, u32>>(),
                g::route(
                    g::seq(
                        g::send::<1, 0, Msg<188, u32>>(),
                        g::send::<0, 1, Msg<184, u32>>(),
                    ),
                    g::seq(
                        g::route(
                            g::send::<1, 0, Msg<182, u32>>(),
                            g::send::<1, 0, Msg<183, u32>>(),
                        ),
                        g::send::<0, 1, Msg<184, u32>>(),
                    ),
                ),
            ),
            g::send::<0, 1, Msg<185, u32>>(),
        )
        .roll(),
        g::seq(
            g::send::<1, 0, Msg<186, u32>>(),
            g::send::<0, 1, Msg<187, u32>>(),
        ),
    )
}

#[test]
fn nested_send_continuation_keeps_the_selected_occurrence_across_visits() {
    let (p0, p1): (RoleProgram<0>, RoleProgram<1>) =
        (project(&nested_reply_path()), project(&nested_reply_path()));
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let carrier = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let mut client = rv.enter(SessionId::new(1), &p0).unwrap();
    let mut owner = rv.enter(SessionId::new(1), &p1).unwrap();
    futures::executor::block_on(async {
        for reply in [182, 183, 188, 183, 182] {
            client.send::<Msg<171, u32>>(&reply).await.unwrap();
            assert_eq!(
                owner
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<171, u32>>()
                    .await
                    .unwrap(),
                reply
            );
            match reply {
                182 => owner.send::<Msg<182, u32>>(&reply).await.unwrap(),
                183 => owner.send::<Msg<183, u32>>(&reply).await.unwrap(),
                188 => owner.send::<Msg<188, u32>>(&reply).await.unwrap(),
                _ => unreachable!(),
            }
            let preview = client.offer().await.unwrap();
            assert_eq!(preview.label(), reply as u8);
            drop(preview);
            let branch = client.offer().await.unwrap();
            let value = match reply {
                182 => branch.recv::<Msg<182, u32>>().await.unwrap(),
                183 => branch.recv::<Msg<183, u32>>().await.unwrap(),
                188 => branch.recv::<Msg<188, u32>>().await.unwrap(),
                _ => unreachable!(),
            };
            assert_eq!(value, reply);
            client.send::<Msg<184, u32>>(&reply).await.unwrap();
            assert_eq!(owner.recv::<Msg<184, u32>>().await.unwrap(), reply);
        }
        client.send::<Msg<185, u32>>(&0).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<185, u32>>()
            .await
            .unwrap();
        owner.send::<Msg<186, u32>>(&0).await.unwrap();
        client.recv::<Msg<186, u32>>().await.unwrap();
        client.send::<Msg<187, u32>>(&0).await.unwrap();
        owner.recv::<Msg<187, u32>>().await.unwrap();
    });
    assert!(carrier.queue_is_empty());
}

#[test]
fn nested_send_continuation_requires_consuming_the_reply() {
    let (p0, p1): (RoleProgram<0>, RoleProgram<1>) =
        (project(&nested_reply_path()), project(&nested_reply_path()));
    let frames = Rc::new(RefCell::new(Vec::new()));
    let carrier = RecordingCarrier {
        inner: TestTransport::new(),
        frames: frames.clone(),
    };
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let mut client = rv.enter(SessionId::new(1), &p0).unwrap();
    let mut owner = rv.enter(SessionId::new(1), &p1).unwrap();
    futures::executor::block_on(async {
        client.send::<Msg<171, u32>>(&0).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<171, u32>>()
            .await
            .unwrap();
        owner.send::<Msg<182, u32>>(&0).await.unwrap();
        drop(client.offer().await.unwrap());
        assert!(client.send::<Msg<184, u32>>(&0).await.is_err());
    });
    assert_eq!(
        frames.borrow().len(),
        2,
        "an unconsumed preview cannot authorize the ack"
    );
}

#[test]
fn nested_send_path_accepts_retire_as_its_first_choice() {
    let (p0, p1): (RoleProgram<0>, RoleProgram<1>) =
        (project(&nested_reply_path()), project(&nested_reply_path()));
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut client = rv.enter(SessionId::new(1), &p0).unwrap();
    let mut owner = rv.enter(SessionId::new(1), &p1).unwrap();
    futures::executor::block_on(async {
        client.send::<Msg<185, u32>>(&0).await.unwrap();
        owner
            .offer()
            .await
            .unwrap()
            .recv::<Msg<185, u32>>()
            .await
            .unwrap();
        owner.send::<Msg<186, u32>>(&0).await.unwrap();
        client.recv::<Msg<186, u32>>().await.unwrap();
        client.send::<Msg<187, u32>>(&0).await.unwrap();
        owner.recv::<Msg<187, u32>>().await.unwrap();
    });
}
#[test]
fn completed_read_then_normal_return_uses_the_outer_entry_with_the_reused_contract() {
    completed_reads_then_return(&[0, 1, 2]);
}

// Miri checks the initial visit and reentry above. Repeating the same prefix
// 64 times is a native stress test, independent of that ownership check.
#[cfg(not(miri))]
#[test]
fn normal_return_after_many_completed_reads_uses_the_outer_entry() {
    completed_reads_then_return(&[64]);
}

fn completed_reads_then_return(read_counts: &[usize]) {
    let global = g::route(
        g::seq(
            g::send::<0, 1, Msg<90, ()>>(),
            g::route(
                g::seq(
                    g::send::<1, 0, Msg<91, ()>>(),
                    g::send::<0, 1, Msg<92, ()>>(),
                ),
                g::seq(
                    g::send::<1, 0, Msg<93, ()>>(),
                    g::seq(
                        g::send::<0, 1, Msg<94, ()>>(),
                        g::send::<1, 0, Msg<95, ()>>(),
                    ),
                ),
            ),
        ),
        g::seq(
            g::send::<0, 1, Msg<94, ()>>(),
            g::send::<1, 0, Msg<95, ()>>(),
        ),
    )
    .roll();
    let p0: RoleProgram<0> = project(&global);
    let p1: RoleProgram<1> = project(&global);
    for &reads in read_counts {
        let mut slab = [0; 8192];
        let mut storage = SessionKitStorage::uninit();
        let kit = storage
            .init()
            .rendezvous(&mut slab, common::TestTransport::new())
            .unwrap();
        let mut requester = kit.enter(SessionId::new(40), &p0).unwrap();
        let mut source = kit.enter(SessionId::new(40), &p1).unwrap();
        futures::executor::block_on(async {
            for _ in 0..reads {
                requester.send::<Msg<90, ()>>(&()).await.unwrap();
                source
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<90, ()>>()
                    .await
                    .unwrap();
                source.send::<Msg<91, ()>>(&()).await.unwrap();
                requester
                    .offer()
                    .await
                    .unwrap()
                    .recv::<Msg<91, ()>>()
                    .await
                    .unwrap();
                requester.send::<Msg<92, ()>>(&()).await.unwrap();
                source.recv::<Msg<92, ()>>().await.unwrap();
            }
            requester.send::<Msg<94, ()>>(&()).await.unwrap();
            source
                .offer()
                .await
                .unwrap()
                .recv::<Msg<94, ()>>()
                .await
                .unwrap();
            source.send::<Msg<95, ()>>(&()).await.unwrap();
            requester.recv::<Msg<95, ()>>().await.unwrap();
        });
    }
}

#[test]
fn an_unfinished_read_cannot_take_the_normal_return_entry() {
    let global = g::route(
        g::seq(
            g::send::<0, 1, Msg<90, ()>>(),
            g::route(
                g::seq(
                    g::send::<1, 0, Msg<91, ()>>(),
                    g::send::<0, 1, Msg<92, ()>>(),
                ),
                g::seq(
                    g::send::<1, 0, Msg<93, ()>>(),
                    g::seq(
                        g::send::<0, 1, Msg<94, ()>>(),
                        g::send::<1, 0, Msg<95, ()>>(),
                    ),
                ),
            ),
        ),
        g::seq(
            g::send::<0, 1, Msg<94, ()>>(),
            g::send::<1, 0, Msg<95, ()>>(),
        ),
    )
    .roll();
    let p0: RoleProgram<0> = project(&global);
    let p1: RoleProgram<1> = project(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut requester = kit.enter(SessionId::new(40), &p0).unwrap();
    let mut source = kit.enter(SessionId::new(40), &p1).unwrap();
    futures::executor::block_on(async {
        requester.send::<Msg<90, ()>>(&()).await.unwrap();
        source
            .offer()
            .await
            .unwrap()
            .recv::<Msg<90, ()>>()
            .await
            .unwrap();
        assert!(requester.send::<Msg<94, ()>>(&()).await.is_err());
    });
}

#[test]
fn intrinsic_choice_can_start_with_either_nested_arm_and_reenter() {
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    )
    .roll();
    let p0: RoleProgram<0> = project(&global);
    let p1: RoleProgram<1> = project(&global);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut sender = kit.enter(SessionId::new(40), &p0).unwrap();
    let mut receiver = kit.enter(SessionId::new(40), &p1).unwrap();
    futures::executor::block_on(async {
        for label in [2, 1, 3, 2, 3, 1] {
            match label {
                1 => sender.send::<Msg<1, ()>>(&()).await.unwrap(),
                2 => sender.send::<Msg<2, ()>>(&()).await.unwrap(),
                3 => sender.send::<Msg<3, ()>>(&()).await.unwrap(),
                _ => unreachable!(),
            }
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), label);
            match label {
                1 => branch.recv::<Msg<1, ()>>().await.unwrap(),
                2 => branch.recv::<Msg<2, ()>>().await.unwrap(),
                3 => branch.recv::<Msg<3, ()>>().await.unwrap(),
                _ => unreachable!(),
            }
        }
    });
}
