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
