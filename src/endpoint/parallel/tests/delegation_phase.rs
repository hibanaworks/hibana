use super::*;
use crate::Endpoint;

type Loan = g::Msg<240, Endpoint<'static, 36>>;
type Install = g::Msg<241, ()>;
type Installed = g::Msg<249, ()>;
type Need = g::Msg<195, ()>;
type Finished = g::Msg<196, ()>;
type Flight = g::Msg<33, u32>;
type Taken = g::Msg<36, ()>;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_file_view_rejects_a_rolled_request_after_resource_retirement()
-> core::result::Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    type FileLoan = g::Msg<201, Endpoint<'static, 3>>;
    type ResourceLoan = g::Msg<202, Endpoint<'static, 2>>;
    type Retire = g::Msg<207, Endpoint<'static, 2>>;
    type Request = g::Msg<203, ()>;
    type Effect = g::Msg<204, ()>;
    type Published = g::Msg<205, ()>;
    type Next = g::Msg<206, ()>;

    for drop_retired in [false, true] {
        let cycle = g::seq(
            g::send::<0, 1, ResourceLoan>(),
            g::seq(
                g::send::<3, 0, Request>(),
                g::seq(g::send::<2, 0, Effect>(), g::send::<1, 0, ResourceLoan>()),
            ),
        );
        let global = g::seq(
            g::send::<0, 1, FileLoan>(),
            g::seq(
                cycle.roll(),
                g::seq(
                    g::send::<0, 4, Retire>(),
                    g::seq(g::send::<4, 3, Published>(), g::send::<4, 1, Next>()),
                ),
            ),
        );
        let p0 = project::<0>(&global);
        let p1 = project::<1>(&global);
        let p2 = project::<2>(&global);
        let p3 = project::<3>(&global);
        let p4 = project::<4>(&global);
        let network = TestTransport::new();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let sid = SessionId::new(1705);
        let mut owner = rv.enter(sid, &p0).unwrap();
        let mut agent = rv.enter(sid, &p1).unwrap();
        let mut finalizer = rv.enter(sid, &p4).unwrap();
        let mut resource = rv.enter(sid, &p2).unwrap();
        let file = rv.enter(sid, &p3).unwrap();
        owner.send::<FileLoan>(file).await?;
        let mut file = agent.recv::<FileLoan>().await?;
        for visit in 0..2 {
            if visit != 0 {
                let mut attempt = core::pin::pin!(file.send::<Request>(&()));
                assert!(
                    attempt
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
                assert!(network.queue_is_empty());
            }
            owner.send::<ResourceLoan>(resource).await?;
            let mut borrowed = agent.recv::<ResourceLoan>().await?;
            file.send::<Request>(&()).await?;
            owner.recv::<Request>().await?;
            borrowed.send::<Effect>(&()).await?;
            owner.recv::<Effect>().await?;
            agent.send::<ResourceLoan>(borrowed).await?;
            resource = owner.recv::<ResourceLoan>().await?;
        }
        owner.send::<Retire>(resource).await?;
        let retired = finalizer.recv::<Retire>().await?;
        finalizer.send::<Published>(&()).await?;
        file.recv::<Published>().await?;
        if drop_retired {
            drop(retired);
        }
        {
            let mut attempt = core::pin::pin!(file.send::<Request>(&()));
            let result = attempt
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()));
            assert!(
                matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
                "retired resource, dropped={drop_retired}: {result:?}"
            );
        }
        {
            let mut peers = ();
            let mut borrowed = file.borrow(&mut peers);
            let mut attempt = core::pin::pin!(borrowed.send::<Request>(&()));
            let result = attempt
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()));
            assert!(
                matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
                "borrowed retired request: {result:?}"
            );
        }
        assert!(network.queue_is_empty());
        finalizer.send::<Next>(&()).await?;
        agent.recv::<Next>().await?;
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_file_view_waits_for_a_rolled_request_after_temporary_resource_transfer()
-> core::result::Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        task::{Context, Waker},
    };
    type FileLoan = g::Msg<201, Endpoint<'static, 3>>;
    type ResourceLoan = g::Msg<202, Endpoint<'static, 2>>;
    type Retire = g::Msg<207, Endpoint<'static, 2>>;
    type Return = g::Msg<208, Endpoint<'static, 2>>;
    type Request = g::Msg<203, ()>;
    type Effect = g::Msg<204, ()>;
    type Published = g::Msg<205, ()>;
    let cycle = g::seq(
        g::send::<0, 1, ResourceLoan>(),
        g::seq(
            g::send::<3, 0, Request>(),
            g::seq(g::send::<2, 0, Effect>(), g::send::<1, 0, ResourceLoan>()),
        ),
    );
    let global = g::seq(
        g::send::<0, 1, FileLoan>(),
        g::seq(
            cycle.roll(),
            g::seq(
                g::send::<0, 4, Retire>(),
                g::seq(g::send::<4, 3, Published>(), g::send::<4, 0, Return>()),
            ),
        ),
    );
    let programs = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
        project::<3>(&global),
        project::<4>(&global),
    );
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1706);
    let mut owner = rv.enter(sid, &programs.0).unwrap();
    let mut agent = rv.enter(sid, &programs.1).unwrap();
    let mut resource = rv.enter(sid, &programs.2).unwrap();
    let file = rv.enter(sid, &programs.3).unwrap();
    let mut temporary = rv.enter(sid, &programs.4).unwrap();
    owner.send::<FileLoan>(file).await?;
    let mut file = agent.recv::<FileLoan>().await?;
    owner.send::<ResourceLoan>(resource).await?;
    let mut held = agent.recv::<ResourceLoan>().await?;
    file.send::<Request>(&()).await?;
    owner.recv::<Request>().await?;
    held.send::<Effect>(&()).await?;
    owner.recv::<Effect>().await?;
    agent.send::<ResourceLoan>(held).await?;
    resource = owner.recv::<ResourceLoan>().await?;
    owner.send::<Retire>(resource).await?;
    let held = temporary.recv::<Retire>().await?;
    temporary.send::<Published>(&()).await?;
    file.recv::<Published>().await?;
    {
        let mut peers = ();
        let mut borrowed = file.borrow(&mut peers);
        let mut attempt = core::pin::pin!(borrowed.send::<Request>(&()));
        assert!(
            attempt
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert!(network.queue_is_empty());
    }
    temporary.send::<Return>(held).await?;
    resource = owner.recv::<Return>().await?;
    owner.send::<ResourceLoan>(resource).await?;
    let mut held = agent.recv::<ResourceLoan>().await?;
    file.send::<Request>(&()).await?;
    owner.recv::<Request>().await?;
    held.send::<Effect>(&()).await?;
    owner.recv::<Effect>().await?;
    agent.send::<ResourceLoan>(held).await?;
    owner.recv::<ResourceLoan>().await?;
    assert!(network.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn an_unvisited_rolled_arm_cannot_wait_for_a_retired_resource()
-> core::result::Result<(), hibana::EndpointError> {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    type File = g::Msg<201, Endpoint<'static, 3>>;
    type Loan = g::Msg<202, Endpoint<'static, 2>>;
    type Retire = g::Msg<207, Endpoint<'static, 2>>;
    type Request = g::Msg<203, ()>;
    type Effect = g::Msg<204, ()>;
    type Published = g::Msg<205, ()>;
    type Left = g::Msg<210, ()>;
    type Right = g::Msg<211, ()>;
    type Next = g::Msg<206, ()>;
    let cycle = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<3, 0, Request>(),
            g::seq(g::send::<2, 0, Effect>(), g::send::<1, 0, Loan>()),
        ),
    );
    let global = g::seq(
        g::send::<0, 1, File>(),
        g::seq(
            g::route(
                g::seq(g::send::<0, 1, Left>(), cycle),
                g::seq(g::send::<0, 1, Right>(), cycle),
            )
            .roll(),
            g::seq(
                g::send::<0, 4, Retire>(),
                g::seq(g::send::<4, 3, Published>(), g::send::<4, 1, Next>()),
            ),
        ),
    );
    let programs = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
        project::<3>(&global),
        project::<4>(&global),
    );
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1707);
    let mut owner = rv.enter(sid, &programs.0).unwrap();
    let mut agent = rv.enter(sid, &programs.1).unwrap();
    let mut resource = rv.enter(sid, &programs.2).unwrap();
    let file = rv.enter(sid, &programs.3).unwrap();
    let mut finalizer = rv.enter(sid, &programs.4).unwrap();
    owner.send::<File>(file).await?;
    let mut file = agent.recv::<File>().await?;
    owner.send::<Left>(&()).await?;
    agent.recv::<Left>().await?;
    owner.send::<Loan>(resource).await?;
    let mut held = agent.recv::<Loan>().await?;
    file.send::<Request>(&()).await?;
    owner.recv::<Request>().await?;
    held.send::<Effect>(&()).await?;
    owner.recv::<Effect>().await?;
    agent.send::<Loan>(held).await?;
    resource = owner.recv::<Loan>().await?;
    owner.send::<Retire>(resource).await?;
    finalizer.recv::<Retire>().await?;
    finalizer.send::<Published>(&()).await?;
    file.recv::<Published>().await?;
    {
        let mut attempt = core::pin::pin!(file.send::<Request>(&()));
        let result = attempt
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        assert!(
            matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
            "unvisited alternative: {result:?}"
        );
    }
    assert!(network.queue_is_empty());
    finalizer.send::<Next>(&()).await?;
    agent.recv::<Next>().await?;
    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_completed_rolled_prefix_allows_its_parallel_continuation()
-> core::result::Result<(), hibana::EndpointError> {
    type Prefix = g::Msg<21, ()>;
    type Ack = g::Msg<22, ()>;
    type Reply = g::Msg<23, u32>;
    let global = g::seq(
        g::seq(g::send::<0, 1, Prefix>(), g::send::<1, 0, Ack>()).roll(),
        g::par(g::send::<0, 1, Flight>(), g::send::<1, 0, Reply>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1668);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        assert!(a.par::<2>().is_err());
        a.send::<Prefix>(&()).await.unwrap();
        b.recv::<Prefix>().await.unwrap();
        assert!(a.par::<2>().is_err());
        b.send::<Ack>(&()).await.unwrap();
        a.recv::<Ack>().await.unwrap();
        let [mut request, mut reply] = a.par().unwrap();
        let [mut receive, mut respond] = b.par().unwrap();
        let (sent, value, received, answered) = async {
            async { Ok::<_, hibana::EndpointError>((request.send::<Flight>(&7)).await) };

            async { Ok::<_, hibana::EndpointError>((reply.recv::<Reply>()).await) };

            async { Ok::<_, hibana::EndpointError>((receive.recv::<Flight>()).await) };

            async { Ok::<_, hibana::EndpointError>((respond.send::<Reply>(&8)).await) };
        }
        .await
        .expect("all child results are data");
        sent.unwrap();
        answered.unwrap();
        assert_eq!(value.unwrap(), 8);
        assert_eq!(received.unwrap(), 7);
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_completed_parallel_roll_stays_in_its_siblings_borrow()
-> core::result::Result<(), hibana::EndpointError> {
    type Prefix = g::Msg<21, ()>;
    type Ack = g::Msg<22, ()>;
    type Reply = g::Msg<23, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, Prefix>(), g::send::<1, 0, Ack>()).roll(),
        g::par(g::send::<0, 1, Flight>(), g::send::<1, 0, Reply>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1667);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        a.send::<Prefix>(&()).await.unwrap();
        b.recv::<Prefix>().await.unwrap();
        b.send::<Ack>(&()).await.unwrap();
        a.recv::<Ack>().await.unwrap();
        assert!(a.par::<2>().is_err());
        let [mut prefix, mut request, mut reply] = a.par().unwrap();
        let [mut receive_prefix, mut receive, mut respond] = b.par().unwrap();
        let ((), (), sent, value, received, answered) = async {
            async {
                let _: () = (async {
                    prefix.send::<Prefix>(&()).await.unwrap();
                    prefix.recv::<Ack>().await.unwrap();
                })
                .await;
                Ok::<_, hibana::EndpointError>(())
            };

            async {
                let _: () = (async {
                    receive_prefix.recv::<Prefix>().await.unwrap();
                    receive_prefix.send::<Ack>(&()).await.unwrap();
                })
                .await;
                Ok::<_, hibana::EndpointError>(())
            };

            async { Ok::<_, hibana::EndpointError>((request.send::<Flight>(&7)).await) };

            async { Ok::<_, hibana::EndpointError>((reply.recv::<Reply>()).await) };

            async { Ok::<_, hibana::EndpointError>((receive.recv::<Flight>()).await) };

            async { Ok::<_, hibana::EndpointError>((respond.send::<Reply>(&8)).await) };
        }
        .await
        .expect("all child results are data");
        sent.unwrap();
        answered.unwrap();
        assert_eq!(value.unwrap(), 8);
        assert_eq!(received.unwrap(), 7);
    };
    assert!(network.queue_is_empty());

    Ok(())
}

fn installation() -> impl crate::runtime::program::Projectable {
    g::seq(
        g::send::<1, 2, Loan>(),
        g::seq(
            g::send::<36, 1, Install>(),
            g::seq(g::send::<1, 36, Installed>(), g::send::<2, 3, Loan>()),
        ),
    )
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn returning_a_resource_can_immediately_borrow_the_next_parallel_phase()
-> core::result::Result<(), hibana::EndpointError> {
    let global = g::seq(
        installation(),
        g::seq(
            g::send::<3, 1, Loan>(),
            g::seq(
                g::par(
                    g::seq(
                        g::send::<1, 0, Need>(),
                        g::seq(g::send::<0, 1, Finished>(), installation()),
                    ),
                    g::seq(g::send::<3, 2, Flight>(), g::send::<2, 3, Taken>()),
                ),
                g::send::<3, 1, Loan>(),
            ),
        ),
    );
    let input_program = project::<0>(&global);
    let owner_program = project::<1>(&global);
    let installer_program = project::<2>(&global);
    let output_program = project::<3>(&global);
    let resource_program = project::<36>(&global);
    let network = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1669);
    let mut input = rv.enter(sid, &input_program).unwrap();
    let mut owner = rv.enter(sid, &owner_program).unwrap();
    let mut installer = rv.enter(sid, &installer_program).unwrap();
    let mut output = rv.enter(sid, &output_program).unwrap();
    let resource = rv.enter(sid, &resource_program).unwrap();
    {
        let start = crate::test_support::in_process::now();
        (async {
            async {
                async {
                    let _: () = (async {
                        input.recv::<Need>().await.unwrap();
                        crate::test_support::in_process::sleep(core::time::Duration::from_secs(1))
                            .await;
                        input.send::<Finished>(&()).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        owner.send::<Loan>(resource).await.unwrap();
                        owner.recv::<Install>().await.unwrap();
                        owner.send::<Installed>(&()).await.unwrap();
                        let resource = owner.recv::<Loan>().await.unwrap();
                        owner.send::<Need>(&()).await.unwrap();
                        owner.recv::<Finished>().await.unwrap();
                        owner.send::<Loan>(resource).await.unwrap();
                        owner.recv::<Install>().await.unwrap();
                        owner.send::<Installed>(&()).await.unwrap();
                        drop(owner.recv::<Loan>().await.unwrap());
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let mut resource = installer.recv::<Loan>().await.unwrap();
                        resource.send::<Install>(&()).await.unwrap();
                        resource.recv::<Installed>().await.unwrap();
                        installer.send::<Loan>(resource).await.unwrap();
                        let [mut installation, mut source] = installer.par().unwrap();
                        async {
                            async {
                                let _: () = (async {
                                    let mut resource = installation.recv::<Loan>().await.unwrap();
                                    resource.send::<Install>(&()).await.unwrap();
                                    resource.recv::<Installed>().await.unwrap();
                                    installation.send::<Loan>(resource).await.unwrap();
                                })
                                .await;
                                Ok::<_, hibana::EndpointError>(())
                            };

                            async {
                                let _: () = (async {
                                    assert_eq!(source.recv::<Flight>().await.unwrap(), 7);
                                    source.send::<Taken>(&()).await.unwrap();
                                })
                                .await;
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let resource = output.recv::<Loan>().await.unwrap();
                        output.send::<Loan>(resource).await.unwrap();
                        let resource = {
                            let [mut installation, mut source] = output.par().unwrap();
                            let (resource, ()) = async {
                                async {
                                    Ok::<_, hibana::EndpointError>(
                                        (async { installation.recv::<Loan>().await.unwrap() })
                                            .await,
                                    )
                                };

                                async {
                                    let _: () = (async {
                                        source.send::<Flight>(&7).await.unwrap();
                                        source.recv::<Taken>().await.unwrap();
                                        assert_eq!(crate::test_support::in_process::now(), start);
                                    })
                                    .await;
                                    Ok::<_, hibana::EndpointError>(())
                                };
                            }
                            .await
                            .expect("all child results are data");
                            resource
                        };
                        output.send::<Loan>(resource).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_parallel_borrow_keeps_its_endpoint_payload_available_for_the_prefix()
-> core::result::Result<(), hibana::EndpointError> {
    type Tick = g::Msg<0, ()>;
    type Read = g::Msg<1, ()>;
    type Loan<'r> = g::Msg<2, Endpoint<'r, 3>>;
    let global = g::par(
        g::send::<0, 0, Tick>().roll(),
        g::seq(g::send::<0, 3, Read>(), g::send::<1, 0, Loan>()),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<3>(&global);
    for send_early in [false, true] {
        for reverse in [false, true] {
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut worker_slab = [0; 32 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut worker_storage = SessionKitStorage::uninit();
            let owner_runtime = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let worker_runtime = worker_storage
                .init()
                .rendezvous(&mut worker_slab, network.clone())
                .unwrap();
            let session = SessionId::new(1703);
            let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
            let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
            let mut resource = worker_runtime.enter(session, &resource_program).unwrap();
            {
                let result = (async {
                    let owner = async {
                        let [mut ticks, mut exchange] = owner.par()?;
                        let ticks = async {
                            ticks.send::<Tick>(&()).await?;
                            hibana_runtime::yield_now().await;
                            ticks.send::<Tick>(&()).await
                        };
                        let exchange = async {
                            exchange.send::<Read>(&()).await?;
                            exchange.recv::<Loan>().await
                        };
                        let (_, resource) = async {
                            async { (ticks).await };
                            async { (exchange).await };
                        }
                        .await?;
                        drop(resource);
                        Ok::<_, crate::EndpointError>(())
                    };
                    let worker = async {
                        crate::test_support::in_process::sleep(core::time::Duration::from_millis(
                            1,
                        ))
                        .await;
                        let [mut transfer] = worker.par()?;
                        if !send_early {
                            resource.recv::<Read>().await?;
                        }
                        transfer.send::<Loan>(resource).await
                    };
                    if reverse {
                        async {
                            async { (worker).await };
                            async { (owner).await };
                        }
                        .await
                        .map(|_| ())
                    } else {
                        async {
                            async { (owner).await };
                            async { (worker).await };
                        }
                        .await
                        .map(|_| ())
                    }
                })
                .await;
                if send_early {
                    result.expect_err("the send future would hold its own prefix");
                } else {
                    result.unwrap();
                    assert!(network.queue_is_empty());
                }
            };
        }
    }
    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_rolled_choice_can_borrow_its_parallel_retirement_while_another_view_finishes()
-> Result<(), crate::EndpointError> {
    type File<'r> = g::Msg<200, Endpoint<'r, 4>>;
    type Work = g::Msg<201, ()>;
    type Ack = g::Msg<202, ()>;
    type FileNotice = g::Msg<203, ()>;
    type PeerNotice = g::Msg<204, ()>;
    type Stop = g::Msg<205, ()>;
    let global = g::seq(
        g::send::<2, 0, File>(),
        g::seq(
            g::route(
                g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()),
                g::par(g::send::<0, 4, FileNotice>(), g::send::<0, 1, PeerNotice>()),
            )
            .roll(),
            g::par(g::send::<0, 2, File>(), g::send::<0, 1, Stop>()),
        ),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p4 = project::<4>(&global);
    for delayed_file_receive in [false, true] {
        let network = TestTransport::new();
        let mut slab = std::vec![0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let session = SessionId::new(991);
        let mut issuer = rv.enter(session, &p2).unwrap();
        let file = rv.enter(session, &p4).unwrap();
        let mut owner = rv.enter(session, &p0).unwrap();
        let mut peer = rv.enter(session, &p1).unwrap();
        issuer.send::<File>(file).await?;
        let mut file = owner.recv::<File>().await?;
        owner.send::<FileNotice>(&()).await?;
        owner.send::<PeerNotice>(&()).await?;
        peer.recv::<PeerNotice>().await?;
        if !delayed_file_receive {
            file.recv::<FileNotice>().await?;
        }
        // SAFETY: this live view was attached with the concrete TestTransport above.
        let snapshot = unsafe { crate::endpoint::proof_snapshot::actor_snapshot(&owner) };
        let [mut retirement, mut stop] = owner.par().unwrap_or_else(|error| {
            panic!("retirement delayed={delayed_file_receive}, {error:?}; {snapshot:#?}")
        });
        if delayed_file_receive {
            let before = rv.tap().checkpoint();
            {
                let value = ();
                let mut pending = core::pin::pin!(stop.send::<Stop>(&value));
                core::future::poll_fn(|cx| {
                    assert!(
                        pending.as_mut().poll(cx).is_pending(),
                        "borrowing cannot discharge the other view's receipt"
                    );
                    core::task::Poll::Ready(())
                })
                .await;
            }
            assert_eq!(
                rv.tap().checkpoint(),
                before,
                "the suspended send must not commit or start transport"
            );
            file.recv::<FileNotice>().await?;
        }
        async {
            async {
                retirement.send::<File>(file).await?;
            };
            async {
                stop.send::<Stop>(&()).await?;
            };
            async {
                drop(issuer.recv::<File>().await?);
            };
            async {
                peer.recv::<Stop>().await?;
            };
            Ok(())
        }
        .await?;
        assert!(network.queue_is_empty());
    }
    Ok(())
}
