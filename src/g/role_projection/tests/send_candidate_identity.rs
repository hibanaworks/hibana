use super::*;
use crate::{
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
    test_support::transport::TestTransport,
};

type Approved = g::Msg<30, ()>;
type Rejected = g::Msg<31, ()>;
type Review = g::Msg<71, ()>;
type Again = g::Msg<77, ()>;
type Intent = g::Msg<89, ()>;
type Start = g::Msg<80, ()>;
type AlternateStart = g::Msg<81, ()>;
type Resource = g::Msg<10, Endpoint<'static, 13>>;
type Io = g::Msg<100, ()>;
type Outcome<M> = g::Seq<
    g::Send<5, 0, M>,
    g::Seq<g::Send<5, 1, M>, g::Seq<g::Send<5, 2, M>, g::Seq<g::Send<5, 6, M>, g::Send<5, 7, M>>>>,
>;
type Announcement<M> = g::Seq<
    g::Send<2, 7, M>,
    g::Seq<g::Send<2, 5, M>, g::Seq<g::Send<2, 6, M>, g::Seq<g::Send<2, 0, M>, g::Send<2, 1, M>>>>,
>;
type Intents = Announcement<Intent>;
type NextReview = g::Send<2, 5, Again>;
type UntilApproved = g::Seq<
    NextReview,
    g::Seq<
        g::Resolve<
            g::Route<
                Outcome<Approved>,
                g::Seq<g::Roll<g::Seq<Outcome<Rejected>, NextReview>>, Outcome<Approved>>,
            >,
            264,
        >,
        Intents,
    >,
>;
type Task = g::Roll<
    g::Seq<
        g::Send<2, 5, Review>,
        g::Resolve<
            g::Route<g::Seq<Outcome<Approved>, Intents>, g::Seq<Outcome<Rejected>, UntilApproved>>,
            264,
        >,
    >,
>;
type Steps = g::Seq<
    g::Send<7, 2, Resource>,
    g::Seq<
        g::Send<2, 7, Resource>,
        g::Par<
            g::Resolve<
                g::Route<
                    g::Seq<Announcement<Start>, Task>,
                    g::Seq<Announcement<AlternateStart>, Task>,
                >,
                260,
            >,
            g::Roll<g::Send<2, 9, Io>>,
        >,
    >,
>;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn resolver_confirmation_preserves_the_admitted_send_occurrence()
-> core::result::Result<(), hibana::EndpointError> {
    {
        let global = g::Program::<Steps>::new();
        let p0 = project::<0>(&global);
        let p1 = project::<1>(&global);
        let p2 = project::<2>(&global);
        let p5 = project::<5>(&global);
        let p6 = project::<6>(&global);
        let p7 = project::<7>(&global);
        let p9 = project::<9>(&global);
        let p13 = project::<13>(&global);
        let network = TestTransport::new();
        let result = core::cell::Cell::new(DecisionArm::Right);
        let selection = DecisionArm::Left;
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let runtime = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        runtime
            .set_resolver(
                &p5,
                ResolverRef::<264>::decision_state(&result, |result| Ok(result.get())),
            )
            .unwrap();
        runtime
            .set_resolver(
                &p2,
                ResolverRef::<260>::decision_state(&selection, |selection| Ok(*selection)),
            )
            .unwrap();
        let session = SessionId::new(6221);
        let mut a = runtime.enter(session, &p0).unwrap();
        let mut b = runtime.enter(session, &p1).unwrap();
        let mut reviewer = runtime.enter(session, &p2).unwrap();
        let mut review = runtime.enter(session, &p5).unwrap();
        let mut other = runtime.enter(session, &p6).unwrap();
        let mut repository = runtime.enter(session, &p7).unwrap();
        let mut io = runtime.enter(session, &p9).unwrap();
        let resource = runtime.enter(session, &p13).unwrap();
        async {
            async {
                let _: () = (async {
                    repository.send::<Resource>(resource).await.unwrap();
                    drop(repository.recv::<Resource>().await.unwrap());
                })
                .await;
                Ok::<_, hibana::EndpointError>(())
            };

            async {
                let _: () = (async {
                    let resource = reviewer.recv::<Resource>().await.unwrap();
                    reviewer.send::<Resource>(resource).await.unwrap();
                })
                .await;
                Ok::<_, hibana::EndpointError>(())
            };
        }
        .await
        .expect("all child results are data");
        let [mut task, mut private] = reviewer.par().unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        task.send::<Start>(&()).await.unwrap();
                        task.send::<Start>(&()).await.unwrap();
                        task.send::<Start>(&()).await.unwrap();
                        task.send::<Start>(&()).await.unwrap();
                        task.send::<Start>(&()).await.unwrap();
                        task.send::<Review>(&()).await.unwrap();
                        task.offer()
                            .await
                            .unwrap()
                            .recv::<Rejected>()
                            .await
                            .unwrap();
                        task.send::<Again>(&()).await.unwrap();
                        task.offer()
                            .await
                            .unwrap()
                            .recv::<Rejected>()
                            .await
                            .unwrap();
                        task.send::<Again>(&()).await.unwrap();
                        task.offer()
                            .await
                            .unwrap()
                            .recv::<Approved>()
                            .await
                            .unwrap();
                        task.offer()
                            .await
                            .unwrap()
                            .send::<Intent>(&())
                            .await
                            .unwrap();
                        task.send::<Intent>(&())
                            .await
                            .expect("second intent recipient");
                        task.send::<Intent>(&())
                            .await
                            .expect("third intent recipient");
                        task.send::<Intent>(&())
                            .await
                            .expect("fourth intent recipient");
                        task.send::<Intent>(&())
                            .await
                            .expect("fifth intent recipient");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        review.recv::<Start>().await.unwrap();
                        review.recv::<Review>().await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.recv::<Again>().await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.send::<Rejected>(&()).await.unwrap();
                        review.recv::<Again>().await.unwrap();
                        result.set(DecisionArm::Left);
                        review.send::<Approved>(&()).await.unwrap();
                        review.send::<Approved>(&()).await.unwrap();
                        review.send::<Approved>(&()).await.unwrap();
                        review.send::<Approved>(&()).await.unwrap();
                        review.send::<Approved>(&()).await.unwrap();
                        review.recv::<Intent>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        a.recv::<Start>().await.unwrap();
                        a.recv::<Rejected>().await.unwrap();
                        a.recv::<Rejected>().await.unwrap();
                        a.recv::<Approved>().await.unwrap();
                        a.recv::<Intent>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        b.recv::<Start>().await.unwrap();
                        b.recv::<Rejected>().await.unwrap();
                        b.recv::<Rejected>().await.unwrap();
                        b.recv::<Approved>().await.unwrap();
                        b.recv::<Intent>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        other.recv::<Start>().await.unwrap();
                        other.recv::<Rejected>().await.unwrap();
                        other.recv::<Rejected>().await.unwrap();
                        other.recv::<Approved>().await.unwrap();
                        other.recv::<Intent>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        repository.recv::<Start>().await.unwrap();
                        repository.recv::<Rejected>().await.unwrap();
                        repository.recv::<Rejected>().await.unwrap();
                        repository.recv::<Approved>().await.unwrap();
                        repository.recv::<Intent>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        private.send::<Io>(&()).await.unwrap();
                        private.send::<Io>(&()).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        io.recv::<Io>().await.unwrap();
                        io.recv::<Io>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
        assert!(network.queue_is_empty());
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn selected_arm_lookup_does_not_replace_a_send_with_a_receive()
-> core::result::Result<(), hibana::EndpointError> {
    {
        type LeftStart = g::Msg<40, ()>;
        type RightStart = g::Msg<41, ()>;
        type Message = g::Msg<42, ()>;
        let global = g::route(
            g::par(g::send::<0, 1, LeftStart>(), g::send::<0, 2, Message>()),
            g::par(
                g::seq(g::send::<0, 1, RightStart>(), g::send::<1, 0, Message>()),
                g::send::<0, 2, Message>(),
            ),
        )
        .resolve::<270>();
        let p0 = project::<0>(&global);
        let p1 = project::<1>(&global);
        let p2 = project::<2>(&global);
        let network = TestTransport::new();
        let selected = DecisionArm::Right;
        let mut slab = [0; 32 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let runtime = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        runtime
            .set_resolver(
                &p0,
                ResolverRef::<270>::decision_state(&selected, |arm| Ok(*arm)),
            )
            .unwrap();
        let session = SessionId::new(6222);
        let mut sender = runtime.enter(session, &p0).unwrap();
        let mut ordered = runtime.enter(session, &p1).unwrap();
        let mut independent = runtime.enter(session, &p2).unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        sender
                            .send::<Message>(&())
                            .await
                            .expect("the independent send in the selected arm is enabled");
                        sender.send::<RightStart>(&()).await.unwrap();
                        sender.recv::<Message>().await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        ordered.recv::<RightStart>().await.unwrap();
                        ordered.send::<Message>(&()).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async { independent.recv::<Message>().await.unwrap() }).await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
        assert!(network.queue_is_empty());
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn visible_input_precedes_a_known_outgoing_continuation()
-> core::result::Result<(), hibana::EndpointError> {
    {
        type Begin = g::Msg<50, ()>;
        type OtherBegin = g::Msg<51, ()>;
        type Input = g::Msg<52, ()>;
        type Ack = g::Msg<53, ()>;
        type Finish = g::Msg<54, ()>;
        type OtherInput = g::Msg<55, ()>;
        let global = g::route(
            g::seq(
                g::send::<1, 0, Begin>(),
                g::seq(
                    g::seq(
                        g::route(g::send::<1, 0, Input>(), g::send::<1, 0, OtherInput>())
                            .resolve::<272>(),
                        g::send::<0, 1, Ack>(),
                    )
                    .roll(),
                    g::send::<0, 1, Finish>(),
                ),
            ),
            g::send::<1, 0, OtherBegin>(),
        )
        .resolve::<271>();
        let p0 = project::<0>(&global);
        let p1 = project::<1>(&global);
        let selected = DecisionArm::Left;
        let network = TestTransport::new();
        let mut slab = [0; 32 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let runtime = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        runtime
            .set_resolver(
                &p1,
                ResolverRef::<271>::decision_state(&selected, |arm| Ok(*arm)),
            )
            .unwrap();
        runtime
            .set_resolver(
                &p1,
                ResolverRef::<272>::decision_state(&selected, |arm| Ok(*arm)),
            )
            .unwrap();
        let session = SessionId::new(6223);
        let mut endpoint = runtime.enter(session, &p0).unwrap();
        let mut peer = runtime.enter(session, &p1).unwrap();
        (async {
            peer.send::<Begin>(&()).await.unwrap();
            endpoint.recv::<Begin>().await.unwrap();
            peer.send::<Input>(&()).await.unwrap();
            endpoint.recv::<Input>().await.unwrap();
            endpoint.send::<Ack>(&()).await.unwrap();
            peer.recv::<Ack>().await.unwrap();

            peer.send::<Input>(&()).await.unwrap();
            let branch = endpoint.offer().await.unwrap();
            assert_eq!(branch.label(), 52, "the actual queued input wins");
            branch.recv::<Input>().await.unwrap();
            endpoint.send::<Ack>(&()).await.unwrap();
            peer.recv::<Ack>().await.unwrap();

            let branch = endpoint.offer().await.unwrap();
            assert_eq!(branch.label(), 54, "no frame is needed for the known send");
            branch.send::<Finish>(&()).await.unwrap();
            peer.recv::<Finish>().await.unwrap();
        })
        .await;
        assert!(network.queue_is_empty());
    };

    Ok(())
}
