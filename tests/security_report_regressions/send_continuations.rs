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
