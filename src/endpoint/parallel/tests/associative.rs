use super::*;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn right_associated_parallel_borrows_have_the_same_flat_local_shape()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<151, u32>;
    type B = g::Msg<152, u32>;
    type C = g::Msg<153, u32>;
    let global = g::par(
        g::send::<0, 1, A>(),
        g::par(g::send::<1, 0, B>(), g::send::<0, 1, C>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1669);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        (async {
            let [mut a_first, mut a_second, mut a_third] = a.par().unwrap();
            let [mut b_first, mut b_second, mut b_third] = b.par().unwrap();
            let (b1, a2, b3, a1, b2, a3) = async {
                async { Ok::<_, hibana::EndpointError>((b_first.recv::<A>()).await) };

                async { Ok::<_, hibana::EndpointError>((a_second.recv::<B>()).await) };

                async { Ok::<_, hibana::EndpointError>((b_third.recv::<C>()).await) };

                async { Ok::<_, hibana::EndpointError>((a_first.send::<A>(&11)).await) };

                async { Ok::<_, hibana::EndpointError>((b_second.send::<B>(&12)).await) };

                async { Ok::<_, hibana::EndpointError>((a_third.send::<C>(&13)).await) };
            }
            .await
            .expect("all child results are data");
            assert_eq!(b1.unwrap(), 11);
            assert_eq!(a2.unwrap(), 12);
            assert_eq!(b3.unwrap(), 13);
            a1.unwrap();
            b2.unwrap();
            a3.unwrap();
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_sequential_prefix_keeps_its_later_parallel_phase()
-> core::result::Result<(), hibana::EndpointError> {
    type Begin = g::Msg<154, ()>;
    type A = g::Msg<155, u32>;
    type B = g::Msg<156, u32>;
    type C = g::Msg<157, u32>;
    let global = g::par(
        g::seq(
            g::send::<0, 1, Begin>(),
            g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        ),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1670);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        let [mut a_work, mut a_tail] = a.par().unwrap();
        let [mut b_work, mut b_tail] = b.par().unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        a_work.send::<Begin>(&()).await.unwrap();
                        let [mut request, mut notice] = a_work.par().unwrap();
                        let (received, sent) = async {
                            async { Ok::<_, hibana::EndpointError>((notice.recv::<B>()).await) };
                            async { Ok::<_, hibana::EndpointError>((request.send::<A>(&7)).await) };
                        }
                        .await
                        .expect("all child results are data");
                        assert_eq!(received.unwrap(), 8);
                        sent.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        b_work.recv::<Begin>().await.unwrap();
                        let [mut request, mut notice] = b_work.par().unwrap();
                        let (received, sent) = async {
                            async { Ok::<_, hibana::EndpointError>((request.recv::<A>()).await) };
                            async { Ok::<_, hibana::EndpointError>((notice.send::<B>(&8)).await) };
                        }
                        .await
                        .expect("all child results are data");
                        assert_eq!(received.unwrap(), 7);
                        sent.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        crate::test_support::in_process::sleep(core::time::Duration::from_secs(1))
                            .await;
                        a_tail.send::<C>(&9).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        assert_eq!(b_tail.recv::<C>().await.unwrap(), 9);
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
async fn a_repeated_parallel_fragment_keeps_its_iteration_boundary()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<158, u32>;
    type B = g::Msg<159, u32>;
    type C = g::Msg<160, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()).roll(),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1671);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        let start = crate::test_support::in_process::now();
        let [mut a_cycles, mut a_tail] = a.par().unwrap();
        let [mut b_cycles, mut b_tail] = b.par().unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        for value in [1, 2, 3] {
                            let [mut request, mut notice] = a_cycles.par().unwrap();
                            let (received, sent) = async {
                                async {
                                    Ok::<_, hibana::EndpointError>((notice.recv::<B>()).await)
                                };
                                async {
                                    Ok::<_, hibana::EndpointError>(
                                        (request.send::<A>(&value)).await,
                                    )
                                };
                            }
                            .await
                            .expect("all child results are data");
                            assert_eq!(received.unwrap(), value + 10);
                            sent.unwrap();
                        }
                        assert!(
                            crate::test_support::in_process::now().saturating_sub(start)
                                < core::time::Duration::from_secs(1)
                        );
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        for value in [1, 2, 3] {
                            let [mut request, mut notice] = b_cycles.par().unwrap();
                            let reply = value + 10;
                            let (received, sent) = async {
                                async {
                                    Ok::<_, hibana::EndpointError>((request.recv::<A>()).await)
                                };
                                async {
                                    Ok::<_, hibana::EndpointError>((notice.send::<B>(&reply)).await)
                                };
                            }
                            .await
                            .expect("all child results are data");
                            assert_eq!(received.unwrap(), value);
                            sent.unwrap();
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        crate::test_support::in_process::sleep(core::time::Duration::from_secs(1))
                            .await;
                        a_tail.send::<C>(&9).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        assert_eq!(b_tail.recv::<C>().await.unwrap(), 9);
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
async fn a_role_without_an_arm_does_not_require_an_extra_borrow()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<161, u32>;
    type Hidden = g::Msg<162, u32>;
    type B = g::Msg<163, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<2, 3, Hidden>()),
        g::send::<1, 0, B>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let c_program = project::<2>(&global);
    let d_program = project::<3>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1672);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    let mut c = rv.enter(sid, &c_program).unwrap();
    let mut d = rv.enter(sid, &d_program).unwrap();
    {
        (async {
            assert!(a.par::<3>().is_err());
            let [mut request, mut notice] = a.par().unwrap();
            let [mut peer_request, mut peer_notice] = b.par().unwrap();
            async {
                async {
                    let _: () = (async {
                        request.send::<A>(&7).await.unwrap();
                        assert_eq!(peer_request.recv::<A>().await.unwrap(), 7);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        peer_notice.send::<B>(&8).await.unwrap();
                        assert_eq!(notice.recv::<B>().await.unwrap(), 8);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        c.send::<Hidden>(&9).await.unwrap();
                        assert_eq!(d.recv::<Hidden>().await.unwrap(), 9);
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
async fn borrowing_parallel_arms_does_not_choose_an_unresolved_route()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<164, u32>;
    type B = g::Msg<165, u32>;
    type C = g::Msg<166, u32>;
    type D = g::Msg<167, u32>;
    type E = g::Msg<168, u32>;
    type F = g::Msg<169, u32>;
    let global = g::route(
        g::par(
            g::send::<0, 1, A>(),
            g::par(g::send::<0, 1, B>(), g::send::<0, 1, C>()),
        ),
        g::par(
            g::send::<0, 1, D>(),
            g::par(g::send::<0, 1, E>(), g::send::<0, 1, F>()),
        ),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1674);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    assert!(
        sender.par::<3>().is_err(),
        "borrowing must not pick a route for its controller"
    );
    assert!(
        receiver.par::<3>().is_err(),
        "borrowing must not guess a route before its first frame"
    );
    {
        (async {
            sender.send::<D>(&17).await.unwrap();
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 167);
            assert_eq!(branch.recv::<D>().await.unwrap(), 17);
            let [sent, mut send_e, mut send_f] = sender.par().unwrap();
            let [received, mut recv_e, mut recv_f] = receiver.par().unwrap();
            drop((sent, received));
            let (e, f, se, sf) = async {
                async { Ok::<_, hibana::EndpointError>((recv_e.recv::<E>()).await) };

                async { Ok::<_, hibana::EndpointError>((recv_f.recv::<F>()).await) };

                async { Ok::<_, hibana::EndpointError>((send_e.send::<E>(&18)).await) };

                async { Ok::<_, hibana::EndpointError>((send_f.send::<F>(&19)).await) };
            }
            .await
            .expect("all child results are data");
            assert_eq!(e.unwrap(), 18);
            assert_eq!(f.unwrap(), 19);
            se.unwrap();
            sf.unwrap();
        })
        .await;
    };
    assert!(network.queue_is_empty());

    Ok(())
}
#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn erased_outer_parallel_arms_do_not_hide_the_next_local_fork()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::{
        g,
        runtime::{SessionKitStorage, ids::SessionId, program::project},
        test_support::transport::TestTransport,
    };
    type Begin = g::Msg<201, u32>;
    type Left = g::Msg<202, u32>;
    type Right = g::Msg<203, u32>;
    type Done = g::Msg<204, u32>;
    type Independent = g::Msg<205, u32>;
    let global = g::par(
        g::seq(
            g::send::<0, 1, Begin>(),
            g::seq(
                g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
                g::send::<1, 0, Done>(),
            ),
        ),
        g::send::<2, 3, Independent>(),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let runtime = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let session = SessionId::new(2610);
    let mut a = runtime.enter(session, &p0).unwrap();
    let mut b = runtime.enter(session, &p1).unwrap();
    let mut c = runtime.enter(session, &p2).unwrap();
    let mut d = runtime.enter(session, &p3).unwrap();
    let (_, _, independent, _) = async {
        async {
            let _: () = (async {
                assert!(a.par::<2>().is_err());
                a.send::<Begin>(&1).await.unwrap();
                assert!(a.par::<4>().is_err());
                {
                    let [mut left, mut right] = a.par().unwrap();
                    let (left, right) = async {
                        async { Ok::<_, hibana::EndpointError>((left.send::<Left>(&2)).await) };
                        async { Ok::<_, hibana::EndpointError>((right.send::<Right>(&3)).await) };
                    }
                    .await
                    .expect("all child results are data");
                    left.unwrap();
                    right.unwrap();
                }
                assert_eq!(a.recv::<Done>().await.unwrap(), 4);
            })
            .await;
            Ok::<_, hibana::EndpointError>(())
        };

        async {
            let _: () = (async {
                let [mut work] = b.par().unwrap();
                assert_eq!(work.recv::<Begin>().await.unwrap(), 1);
                {
                    let [mut left, mut right] = work.par().unwrap();
                    let (right, left) = async {
                        async { Ok::<_, hibana::EndpointError>((right.recv::<Right>()).await) };
                        async { Ok::<_, hibana::EndpointError>((left.recv::<Left>()).await) };
                    }
                    .await
                    .expect("all child results are data");
                    assert_eq!((left.unwrap(), right.unwrap()), (2, 3));
                }
                work.send::<Done>(&4).await.unwrap();
            })
            .await;
            Ok::<_, hibana::EndpointError>(())
        };

        async { Ok::<_, hibana::EndpointError>((c.send::<Independent>(&5)).await) };

        async {
            let _: () = (async {
                assert_eq!(d.recv::<Independent>().await.unwrap(), 5);
            })
            .await;
            Ok::<_, hibana::EndpointError>(())
        };
    }
    .await
    .expect("all child results are data");
    independent.unwrap();
    assert!(network.queue_is_empty());

    Ok(())
}
