use super::*;

#[test]
fn phase_packet_cannot_reenter_a_previous_unchosen_suffix() {
    let choreo = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<0, u32>>(),
                g::seq(
                    g::route(
                        g::send::<1, 0, Msg<180, u32>>(),
                        g::send::<1, 0, Msg<200, u32>>(),
                    ),
                    g::seq(
                        g::route(
                            g::send::<1, 0, Msg<190, u32>>(),
                            g::send::<1, 0, Msg<210, u32>>(),
                        ),
                        g::send::<0, 1, Msg<162, u32>>(),
                    ),
                ),
            ),
            g::seq(
                g::send::<0, 1, Msg<1, u32>>(),
                g::seq(
                    g::send::<1, 0, Msg<165, u32>>(),
                    g::send::<0, 1, Msg<162, u32>>(),
                ),
            ),
        )
        .roll(),
        g::seq(
            g::send::<1, 0, Msg<2, u32>>(),
            g::route(
                g::seq(
                    g::send::<0, 1, Msg<8, u32>>(),
                    g::seq(
                        g::route(
                            g::send::<1, 0, Msg<181, u32>>(),
                            g::send::<1, 0, Msg<201, u32>>(),
                        ),
                        g::seq(
                            g::route(
                                g::send::<1, 0, Msg<191, u32>>(),
                                g::send::<1, 0, Msg<211, u32>>(),
                            ),
                            g::send::<0, 1, Msg<162, u32>>(),
                        ),
                    ),
                ),
                g::seq(
                    g::send::<0, 1, Msg<9, u32>>(),
                    g::seq(
                        g::send::<1, 0, Msg<165, u32>>(),
                        g::send::<0, 1, Msg<162, u32>>(),
                    ),
                ),
            )
            .roll(),
        ),
    );
    let (p0, p1): (RoleProgram<0>, RoleProgram<1>) = (project(&choreo), project(&choreo));
    let mut phase_colors = Vec::new();
    for old_suffix_consumed in [false, true] {
        let frames = Rc::new(RefCell::new(Vec::new()));
        let carrier = RecordingCarrier {
            inner: TestTransport::new(),
            frames: frames.clone(),
        };
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
        let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
        let (mut client, mut owner) = (
            rv.enter(SessionId::new(1), &p0).unwrap(),
            rv.enter(SessionId::new(1), &p1).unwrap(),
        );
        futures::executor::block_on(async {
            futures::try_join!(
                async {
                    client.send::<Msg<0, u32>>(&10).await?;
                    assert_eq!(client.offer().await?.recv::<Msg<200, u32>>().await?, 10);
                    let branch = client.offer().await?;
                    if old_suffix_consumed {
                        assert_eq!(branch.recv::<Msg<210, u32>>().await?, 10);
                    } else {
                        assert_eq!(branch.recv::<Msg<190, u32>>().await?, 10);
                    }
                    client.send::<Msg<162, u32>>(&10).await?;
                    assert_eq!(client.recv::<Msg<2, u32>>().await?, 10);
                    client.send::<Msg<8, u32>>(&11).await?;
                    assert_eq!(client.offer().await?.recv::<Msg<181, u32>>().await?, 11);
                    let branch = client.offer().await?;
                    assert_eq!(
                        branch.label(),
                        211,
                        "a current packet must retain its current phase owner"
                    );
                    drop(branch);
                    assert_eq!(client.offer().await?.recv::<Msg<211, u32>>().await?, 11);
                    client.send::<Msg<162, u32>>(&11).await?;
                    Ok::<_, hibana::EndpointError>(())
                },
                async {
                    assert_eq!(owner.offer().await?.recv::<Msg<0, u32>>().await?, 10);
                    owner.send::<Msg<200, u32>>(&10).await?;
                    if old_suffix_consumed {
                        owner.send::<Msg<210, u32>>(&10).await?;
                    } else {
                        owner.send::<Msg<190, u32>>(&10).await?;
                    }
                    assert_eq!(owner.recv::<Msg<162, u32>>().await?, 10);
                    owner.send::<Msg<2, u32>>(&10).await?;
                    assert_eq!(owner.offer().await?.recv::<Msg<8, u32>>().await?, 11);
                    owner.send::<Msg<181, u32>>(&11).await?;
                    owner.send::<Msg<211, u32>>(&11).await?;
                    assert_eq!(owner.recv::<Msg<162, u32>>().await?, 11);
                    Ok::<_, hibana::EndpointError>(())
                }
            )
            .unwrap();
        });
        let frames = frames.borrow();
        assert_eq!(frames.iter().filter(|f| f.target == 0).count(), 5);
        let current = frames.iter().rfind(|f| f.target == 0).unwrap();
        if old_suffix_consumed {
            let old = frames.iter().filter(|f| f.target == 0).nth(1).unwrap();
            // The completed old roll remains eligible to reenter. Reusing its
            // full inbound identity would make the current packet ambiguous.
            assert_eq!(old.lane, current.lane);
            assert_ne!(
                old.label, current.label,
                "different elastic owners require distinct inbound wire colors"
            );
        }
        phase_colors.push((current.lane, current.label));
    }
    assert_eq!(phase_colors[0], phase_colors[1]);
}

#[test]
fn nested_three_result_route_reenters_after_a_fresh_request() {
    let choreo = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<0, u32>>(),
                g::seq(
                    g::route(
                        g::send::<1, 0, Msg<190, u32>>(),
                        g::route(
                            g::send::<1, 0, Msg<180, u32>>(),
                            g::send::<1, 0, Msg<200, u32>>(),
                        ),
                    ),
                    g::send::<0, 1, Msg<162, u32>>(),
                ),
            ),
            g::seq(
                g::send::<0, 1, Msg<1, u32>>(),
                g::send::<1, 0, Msg<165, u32>>(),
            ),
        )
        .roll(),
        g::send::<1, 0, Msg<2, u32>>(),
    );
    let (p0, p1): (RoleProgram<0>, RoleProgram<1>) = (project(&choreo), project(&choreo));
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let (mut client, mut owner) = (
        rv.enter(SessionId::new(1), &p0).unwrap(),
        rv.enter(SessionId::new(1), &p1).unwrap(),
    );
    futures::executor::block_on(async {
        futures::try_join!(
            async {
                for n in 0..2 {
                    client.send::<Msg<0, u32>>(&n).await?;
                    let b = client.offer().await?;
                    if n == 0 {
                        assert_eq!(b.recv::<Msg<180, u32>>().await?, n);
                    } else {
                        assert_eq!(b.recv::<Msg<200, u32>>().await?, n);
                    }
                    client.send::<Msg<162, u32>>(&n).await?;
                }
                Ok::<_, hibana::EndpointError>(())
            },
            async {
                for n in 0..2 {
                    assert_eq!(owner.offer().await?.recv::<Msg<0, u32>>().await?, n);
                    if n == 0 {
                        owner.send::<Msg<180, u32>>(&n).await?;
                    } else {
                        owner.send::<Msg<200, u32>>(&n).await?;
                    }
                    assert_eq!(owner.recv::<Msg<162, u32>>().await?, n);
                }
                Ok::<_, hibana::EndpointError>(())
            }
        )
        .unwrap();
    });
}

fn rolled_adjacent_choices() -> impl Projectable {
    g::seq(
        g::route(
            g::send::<0, 1, Msg<180, u32>>(),
            g::send::<0, 1, Msg<200, u32>>(),
        ),
        g::route(
            g::send::<0, 1, Msg<190, u32>>(),
            g::send::<0, 1, Msg<210, u32>>(),
        ),
    )
    .roll()
}

fn capture_adjacent_choices(first_left: bool, second_left: bool) -> Vec<Captured> {
    let frames = Rc::new(RefCell::new(Vec::new()));
    let carrier = RecordingCarrier {
        inner: TestTransport::new(),
        frames: frames.clone(),
    };
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<RecordingCarrier>::uninit();
    let rv = storage.init().rendezvous(&mut slab, carrier).unwrap();
    let program: RoleProgram<0> = project(&rolled_adjacent_choices());
    let mut origin = rv.enter(SessionId::new(1), &program).unwrap();
    futures::executor::block_on(async {
        if first_left {
            origin.send::<Msg<180, u32>>(&18).await.unwrap();
        } else {
            origin.send::<Msg<200, u32>>(&20).await.unwrap();
        }
        if second_left {
            origin.send::<Msg<190, u32>>(&19).await.unwrap();
        } else {
            origin.send::<Msg<210, u32>>(&21).await.unwrap();
        }
    });
    frames.borrow().clone()
}

#[test]
fn rolled_adjacent_offers_accept_each_sibling_arm_without_an_extra_ack() {
    for first_left in [true, false] {
        for second_left in [true, false] {
            let frames = capture_adjacent_choices(first_left, second_left);
            let carrier = TestTransport::new();
            let mut slab = [0; 128 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, carrier.clone())
                .unwrap();
            let program: RoleProgram<1> = project(&rolled_adjacent_choices());
            let mut receiver = rv.enter(SessionId::new(1), &program).unwrap();
            for frame in &frames {
                inject(&carrier, 0, frame);
            }
            futures::executor::block_on(async {
                let branch = receiver.offer().await.unwrap();
                if first_left {
                    assert_eq!(branch.recv::<Msg<180, u32>>().await.unwrap(), 18);
                } else {
                    assert_eq!(branch.recv::<Msg<200, u32>>().await.unwrap(), 20);
                }
                // Dropping a preview must preserve the exact same branch and payload.
                let expected = if second_left { 190 } else { 210 };
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), expected);
                drop(branch);
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), expected);
                if second_left {
                    assert_eq!(branch.recv::<Msg<190, u32>>().await.unwrap(), 19);
                } else {
                    assert_eq!(branch.recv::<Msg<210, u32>>().await.unwrap(), 21);
                }
            });
            assert!(carrier.queue_is_empty());
        }
    }
}

#[test]
fn rolled_adjacent_offer_rejects_wrong_source_lane_or_frame_color() {
    for corrupt in 0..3 {
        let mut frames = capture_adjacent_choices(true, false);
        let carrier = TestTransport::new();
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, carrier.clone())
            .unwrap();
        let program: RoleProgram<1> = project(&rolled_adjacent_choices());
        let mut receiver = rv.enter(SessionId::new(1), &program).unwrap();
        inject(&carrier, 0, &frames[0]);
        futures::executor::block_on(async {
            receiver
                .offer()
                .await
                .unwrap()
                .recv::<Msg<180, u32>>()
                .await
                .unwrap();
        });
        let source = if corrupt == 0 { 2 } else { 0 };
        if corrupt == 1 {
            frames[1].lane = 7;
        }
        if corrupt == 2 {
            frames[1].label = 255;
        }
        inject(&carrier, source, &frames[1]);
        let mut offer = pin!(receiver.offer());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(
            !matches!(offer.as_mut().poll(&mut cx), Poll::Ready(Ok(_))),
            "corrupt full frame key must not authorize a route"
        );
    }
}

#[test]
fn rolled_adjacent_offer_cannot_skip_the_first_route() {
    let frames = capture_adjacent_choices(true, false);
    let carrier = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, carrier.clone())
        .unwrap();
    let program: RoleProgram<1> = project(&rolled_adjacent_choices());
    let mut receiver = rv.enter(SessionId::new(1), &program).unwrap();
    inject(&carrier, 0, &frames[1]);
    let mut offer = pin!(receiver.offer());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(
        !matches!(offer.as_mut().poll(&mut cx), Poll::Ready(Ok(_))),
        "exact later frame must not bypass a live earlier route"
    );
}
