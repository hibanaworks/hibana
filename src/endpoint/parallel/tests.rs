use crate::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

mod associative;
mod choice;
mod delegation_phase;
#[cfg(not(miri))]
mod operation_cut;
mod reentrant;

#[test]
fn the_immutable_lane_reference_keeps_parallel_handles_and_futures_compact() {
    use crate::endpoint::carrier::{OperationHandle, OperationRegion};
    use core::mem::size_of;
    assert_eq!(size_of::<OperationRegion>(), 24);
    assert_eq!(size_of::<OperationHandle>(), 32);
    assert_eq!(
        size_of::<crate::ParallelEndpoint<'static, 'static, 0>>(),
        size_of::<usize>() + 32
    );
    assert!(size_of::<crate::Endpoint<'static, 0>>() <= 3 * size_of::<usize>());
    std::println!(
        "parallel region={} operation_handle={} endpoint={} parallel_endpoint={}",
        size_of::<OperationRegion>(),
        size_of::<OperationHandle>(),
        size_of::<crate::Endpoint<'static, 0>>(),
        size_of::<crate::ParallelEndpoint<'static, 'static, 0>>()
    );
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn identical_message_types_remain_independent_in_their_own_arms()
-> core::result::Result<(), hibana::EndpointError> {
    type Value = g::Msg<121, u32>;
    let global = g::par(g::send::<0, 1, Value>(), g::send::<2, 1, Value>());
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let c_program = project::<2>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1651);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    let mut c = rv.enter(sid, &c_program).unwrap();
    {
        let [mut b_left, mut b_right] = b.par().unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        c.send::<Value>(&22).await.unwrap();
                        assert_eq!(b_right.recv::<Value>().await.unwrap(), 22);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        crate::test_support::in_process::sleep(core::time::Duration::from_secs(1))
                            .await;
                        a.send::<Value>(&11).await.unwrap();
                        assert_eq!(b_left.recv::<Value>().await.unwrap(), 11);
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
async fn parallel_arms_poll_independently_and_join_the_same_parent()
-> core::result::Result<(), hibana::EndpointError> {
    type Start = g::Msg<41, u32>;
    type Ack = g::Msg<42, u32>;
    type Done = g::Msg<43, u32>;
    let global = g::seq(
        g::par(g::send::<0, 1, Start>(), g::send::<1, 0, Ack>()),
        g::send::<0, 1, Done>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1635);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    let a_tag = crate::endpoint::proof_snapshot::view_tag(&a);
    let mut operation_cuts = std::vec::Vec::new();
    {
        (async {
            {
                let [mut a_left, mut a_right] = a.par().unwrap();
                let [mut b_left, mut b_right] = b.par().unwrap();
                let left_tag = a_left.proof_tag();
                let right_tag = a_right.proof_tag();
                async {
                    async {
                        let _: () = (async {
                            assert_eq!(a_right.recv::<Ack>().await.unwrap(), 8);
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            assert_eq!(b_left.recv::<Start>().await.unwrap(), 7);
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            crate::test_support::in_process::sleep(
                                core::time::Duration::from_secs(1),
                            )
                            .await;
                            // SAFETY: these copied tags refer to this pinned
                            // TestTransport actor, between synchronous polls.
                            let (whole, left, right) = unsafe {
                                (
                                    a_tag.operation_cut::<TestTransport>(),
                                    left_tag.operation_cut::<TestTransport>(),
                                    right_tag.operation_cut::<TestTransport>(),
                                )
                            };
                            use crate::endpoint::kernel::PublicActiveOp;
                            assert_eq!(whole.actor.views[0].operation, "Idle");
                            assert!(whole.target.other_active);
                            assert!(!left.target.other_active);
                            assert_eq!(right.actor.views[0].operation, "Idle");
                            assert_eq!(
                                right
                                    .operations
                                    .iter()
                                    .find(|slot| slot.view == right.target.view
                                        && slot.lane == right.target.lane)
                                    .unwrap()
                                    .phase,
                                PublicActiveOp::Recv
                            );
                            assert_eq!(whole.operations, left.operations);
                            assert_eq!(left.operations, right.operations);
                            operation_cuts.extend([whole, left, right]);
                            a_left.send::<Start>(&7).await.unwrap();
                            // SAFETY: the send completed, and the sibling's
                            // receive remains pending until the later Ack.
                            operation_cuts
                                .push(unsafe { left_tag.operation_cut::<TestTransport>() });
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            crate::test_support::in_process::sleep(
                                core::time::Duration::from_secs(2),
                            )
                            .await;
                            b_right.send::<Ack>(&8).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await
                .expect("all child results are data");
            }
            let (sent, received) = async {
                async { Ok::<_, hibana::EndpointError>((a.send::<Done>(&9)).await) };
                async { Ok::<_, hibana::EndpointError>((b.recv::<Done>()).await) };
            }
            .await
            .expect("all child results are data");
            sent.unwrap();
            assert_eq!(received.unwrap(), 9);
        })
        .await;
    };
    assert!(network.queue_is_empty());
    assert_eq!(operation_cuts.len(), 4);
    #[cfg(not(miri))]
    operation_cut::write_observations(&a_program, &operation_cuts);

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_sibling_fault_preserves_a_borrowed_payload()
-> core::result::Result<(), hibana::EndpointError> {
    type Data = g::Msg<51, &'static [u8]>;
    type Good = g::Msg<52, u32>;
    type Wrong = g::Msg<52, u64>;
    let global = g::par(g::send::<0, 1, Data>(), g::send::<0, 1, Good>());
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1636);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        let [mut a_left, mut a_right] = a.par().unwrap();
        let [mut b_left, mut b_right] = b.par().unwrap();
        a_left
            .send::<Data>(&b"borrow survives the other lane".as_slice())
            .await
            .unwrap();
        let data = b_left.recv::<Data>().await.unwrap();
        a_right.send::<Good>(&7).await.unwrap();
        assert!(b_right.recv::<Wrong>().await.is_err());
        assert_eq!(data, b"borrow survives the other lane");
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn left_associated_parallel_borrows_have_a_flat_local_shape()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<61, u32>;
    type B = g::Msg<62, u32>;
    type C = g::Msg<63, u32>;
    let global = g::par(
        g::par(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::send::<0, 1, C>(),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1637);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        (async {
            assert!(
                a.par::<2>().is_err(),
                "a missing arm cannot escape as a borrow"
            );
            assert!(a.par::<4>().is_err(), "an extra arm cannot be invented");
            let [mut a_first, mut a_second, mut a_right] = a.par().unwrap();
            let [mut b_first, mut b_second, mut b_right] = b.par().unwrap();
            let (a1, b1, a2, b2, a3, b3) = async {
                async { Ok::<_, hibana::EndpointError>((a_first.send::<A>(&11)).await) };

                async { Ok::<_, hibana::EndpointError>((b_first.recv::<A>()).await) };

                async { Ok::<_, hibana::EndpointError>((a_second.recv::<B>()).await) };

                async { Ok::<_, hibana::EndpointError>((b_second.send::<B>(&12)).await) };

                async { Ok::<_, hibana::EndpointError>((a_right.send::<C>(&13)).await) };

                async { Ok::<_, hibana::EndpointError>((b_right.recv::<C>()).await) };
            }
            .await
            .expect("all child results are data");
            a1.unwrap();
            assert_eq!(b1.unwrap(), 11);
            assert_eq!(a2.unwrap(), 12);
            b2.unwrap();
            a3.unwrap();
            assert_eq!(b3.unwrap(), 13);
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn parallel_route_previews_do_not_take_the_sibling_choice()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<71, u32>;
    type B = g::Msg<72, u32>;
    type C = g::Msg<73, u32>;
    type D = g::Msg<74, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::route(g::send::<1, 0, C>(), g::send::<1, 0, D>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1638);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        (async {
            let [mut a_left, mut a_right] = a.par().unwrap();
            let [mut b_left, mut b_right] = b.par().unwrap();
            async {
                async {
                    let _: () = (async {
                        let branch = a_right.offer().await.unwrap();
                        assert_eq!(branch.label(), 74);
                        assert_eq!(branch.recv::<D>().await.unwrap(), 14);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let branch = b_left.offer().await.unwrap();
                        assert_eq!(branch.label(), 71);
                        assert_eq!(branch.recv::<A>().await.unwrap(), 11);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        a_left.send::<A>(&11).await.unwrap();
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        b_right.send::<D>(&14).await.unwrap();
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

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_rolled_parallel_body_reenters_after_both_arms_join()
-> core::result::Result<(), hibana::EndpointError> {
    type A = g::Msg<81, u32>;
    type B = g::Msg<82, u32>;
    type C = g::Msg<83, u32>;
    type D = g::Msg<84, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<0, 1, C>(), g::send::<1, 0, D>()),
    )
    .roll();
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1639);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        (async {
            for number in [1, 2, 3] {
                let [mut a_left, mut a_right] = a.par().unwrap();
                let [mut b_left, mut b_right] = b.par().unwrap();
                async {
                    async {
                        let _: () = (async {
                            a_left.send::<A>(&number).await.unwrap();
                            assert_eq!(a_left.recv::<B>().await.unwrap(), number + 1);
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            a_right.send::<C>(&number).await.unwrap();
                            assert_eq!(a_right.recv::<D>().await.unwrap(), number + 2);
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            let value = b_left.recv::<A>().await.unwrap();
                            b_left.send::<B>(&(value + 1)).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            let value = b_right.recv::<C>().await.unwrap();
                            b_right.send::<D>(&(value + 2)).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await
                .expect("all child results are data");
            }
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_fault_wakes_a_pending_sibling_on_the_same_endpoint()
-> core::result::Result<(), hibana::EndpointError> {
    type Left = g::Msg<81, u32>;
    type Good = g::Msg<82, u32>;
    type Wrong = g::Msg<82, u64>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Good>());
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1643);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        let [_a_left, mut a_right] = a.par().unwrap();
        let [mut b_left, mut b_right] = b.par().unwrap();
        (async {
            let (left, right) = async {
                async { Ok::<_, hibana::EndpointError>((b_left.recv::<Left>()).await) };
                async {
                    Ok::<_, hibana::EndpointError>(
                        (async {
                            a_right.send::<Good>(&7).await.unwrap();
                            b_right.recv::<Wrong>().await
                        })
                        .await,
                    )
                };
            }
            .await
            .expect("all child results are data");
            assert!(left.is_err());
            assert!(right.is_err());
        })
        .await;
    };

    Ok(())
}

#[test]
fn forgotten_arm_receive_is_retired_with_the_parent() {
    type Left = g::Msg<83, u32>;
    type Right = g::Msg<84, u32>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<0, 1, Right>());
    let b_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let mut b = rv.enter(SessionId::new(1644), &b_program).unwrap();
    {
        let [left, mut right] = b.par().unwrap();
        let mut pending = right.recv::<Right>();
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        assert!(
            core::future::Future::poll(core::pin::Pin::new(&mut pending), &mut cx).is_pending()
        );
        // Safe Rust can abandon a future and its arm without running Drop.
        core::mem::forget(pending);
        core::mem::forget(left);
        core::mem::forget(right);
    }
    drop(b);
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn one_arm_repeats_while_the_other_arm_is_still_waiting()
-> core::result::Result<(), hibana::EndpointError> {
    type Request = g::Msg<111, u32>;
    type Reply = g::Msg<112, u32>;
    type Slow = g::Msg<113, u32>;
    type Done = g::Msg<114, u32>;
    let global = g::par(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()).roll(),
        g::seq(g::send::<0, 1, Slow>(), g::send::<1, 0, Done>()),
    );
    let a_program = project::<0>(&global);
    let b_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(1647);
    let mut a = rv.enter(sid, &a_program).unwrap();
    let mut b = rv.enter(sid, &b_program).unwrap();
    {
        let start = crate::test_support::in_process::now();
        let [mut a_left, mut a_right] = a.par().unwrap();
        let [mut b_left, mut b_right] = b.par().unwrap();
        (async {
            async {
                async {
                    let _: () = (async {
                        for value in [1, 2, 3] {
                            a_left.send::<Request>(&value).await.unwrap();
                            assert_eq!(a_left.recv::<Reply>().await.unwrap(), value + 1);
                        }
                        assert!(
                            crate::test_support::in_process::now().saturating_sub(start)
                                < core::time::Duration::from_secs(1),
                            "the rolled arm must finish before its sibling's delayed send"
                        );
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        for _ in 0..3 {
                            let value = b_left.recv::<Request>().await.unwrap();
                            b_left.send::<Reply>(&(value + 1)).await.unwrap();
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        crate::test_support::in_process::sleep(core::time::Duration::from_secs(1))
                            .await;
                        a_right.send::<Slow>(&7).await.unwrap();
                        assert_eq!(a_right.recv::<Done>().await.unwrap(), 8);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let value = b_right.recv::<Slow>().await.unwrap();
                        b_right.send::<Done>(&(value + 1)).await.unwrap();
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

    Ok(())
}
