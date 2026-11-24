//! A different branch's possible holder cannot supply a retained operation.
#[path = "../examples/support/in_memory.rs"]
mod in_memory;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    Endpoint, g,
    g::Message,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use in_memory::InMemoryTransport;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_two_resource_retry_keeps_its_incoming_continuation_after_parallel_returns() {
    use core::cell::Cell;
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type First<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Second<'r> = g::Msg<2, Endpoint<'r, 4>>;
    type Passed = g::Msg<3, ()>;
    type Failed = g::Msg<4, ()>;
    type Following = g::Msg<5, ()>;
    type OtherPassed = g::Msg<6, ()>;
    type FirstWork = g::Msg<7, ()>;
    type SecondWork = g::Msg<8, ()>;
    type LastPassed = g::Msg<9, ()>;
    type LastFailed = g::Msg<10, ()>;
    type OtherFailed = g::Msg<11, ()>;
    let submitted = || {
        g::seq(
            g::par(g::send::<3, 0, FirstWork>(), g::send::<4, 0, SecondWork>()),
            g::par(g::send::<1, 0, First>(), g::send::<1, 0, Second>()),
        )
    };
    let returned = || g::par(g::send::<0, 1, First>(), g::send::<0, 1, Second>());
    let verdict =
        || g::route(g::send::<0, 1, LastPassed>(), g::send::<0, 1, LastFailed>()).resolve::<9181>();
    let finished = || {
        g::route(
            g::seq(
                g::send::<0, 1, OtherPassed>(),
                g::seq(
                    returned(),
                    g::seq(
                        g::send::<1, 0, Following>(),
                        g::seq(
                            g::send::<4, 0, SecondWork>(),
                            g::seq(g::send::<1, 0, Second>(), verdict()),
                        ),
                    ),
                ),
            ),
            g::seq(
                g::send::<0, 1, Passed>(),
                g::seq(
                    returned(),
                    g::seq(
                        g::send::<1, 0, Following>(),
                        g::seq(
                            g::send::<3, 0, FirstWork>(),
                            g::seq(g::send::<1, 0, First>(), verdict()),
                        ),
                    ),
                ),
            ),
        )
        .resolve::<9182>()
    };
    let global = g::seq(
        returned(),
        g::seq(
            submitted(),
            g::route(
                finished(),
                g::seq(
                    g::seq(
                        g::route(g::send::<0, 1, OtherFailed>(), g::send::<0, 1, Failed>())
                            .resolve::<9182>(),
                        g::seq(returned(), submitted()),
                    )
                    .roll(),
                    finished(),
                ),
            )
            .resolve::<9181>(),
        ),
    );
    let owner_program = project::<0>(&global);
    let client_program = project::<1>(&global);
    let first_program = project::<3>(&global);
    let second_program = project::<4>(&global);
    for (failures, pending_first) in [
        (1, false),
        (1, true),
        (2, false),
        (2, true),
        (4, false),
        (4, true),
    ] {
        let sid = SessionId::new(9190 + failures);
        // SAFETY: these are the sole bindings of two distinct fixture resources.
        let network = InMemoryTransport::new(unsafe {
            [
                ((sid, 3), EndpointBinding::new()),
                ((sid, 4), EndpointBinding::new()),
            ]
        });
        let actual = Cell::new(DecisionArm::Right);
        let selected = Cell::new(DecisionArm::Right);
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage.init().rendezvous(&mut slab, network).unwrap();
        rv.set_resolver(
            &owner_program,
            ResolverRef::<9181>::decision_state(&actual, |value: &Cell<DecisionArm>| {
                Ok(value.get())
            }),
        )
        .unwrap();
        rv.set_resolver(
            &owner_program,
            ResolverRef::<9182>::decision_state(&selected, |value: &Cell<DecisionArm>| {
                Ok(value.get())
            }),
        )
        .unwrap();
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut client = rv.enter(sid, &client_program).unwrap();
        let first = rv.enter(sid, &first_program).unwrap();
        let second = rv.enter(sid, &second_program).unwrap();
        async {
            let [mut a, mut b] = owner.par().unwrap();
            async {
                a.send::<First>(first).await.unwrap();
                Ok(())
            };
            async {
                b.send::<Second>(second).await.unwrap();
                Ok(())
            };
            Ok(())
        }
        .await
        .unwrap();
        let mut first = client.recv::<First>().await.unwrap();
        let mut second = client.recv::<Second>().await.unwrap();
        for round in 0..=failures {
            async {
                async {
                    first.send::<FirstWork>(&()).await.unwrap();
                    Ok(())
                };
                async {
                    second.send::<SecondWork>(&()).await.unwrap();
                    Ok(())
                };
                Ok(())
            }
            .await
            .unwrap();
            owner.recv::<FirstWork>().await.unwrap();
            owner.recv::<SecondWork>().await.unwrap();
            async {
                let [mut a, mut b] = client.par().unwrap();
                async {
                    a.send::<First>(first).await.unwrap();
                    Ok(())
                };
                async {
                    b.send::<Second>(second).await.unwrap();
                    Ok(())
                };
                Ok(())
            }
            .await
            .unwrap();
            let returned_first = owner.recv::<First>().await.unwrap();
            let returned_second = owner.recv::<Second>().await.unwrap();
            if round == failures {
                actual.set(DecisionArm::Left);
                owner.send::<Passed>(&()).await.unwrap();
                client.recv::<Passed>().await.unwrap();
            } else {
                owner.send::<Failed>(&()).await.unwrap();
                client.recv::<Failed>().await.unwrap();
            }
            async {
                let [mut a, mut b] = owner.par().unwrap();
                async {
                    a.send::<First>(returned_first).await.unwrap();
                    Ok(())
                };
                async {
                    b.send::<Second>(returned_second).await.unwrap();
                    Ok(())
                };
                Ok(())
            }
            .await
            .unwrap();
            first = client.recv::<First>().await.unwrap();
            second = client.recv::<Second>().await.unwrap();
        }
        {
            if !pending_first {
                client.send::<Following>(&()).await.unwrap();
            }
            let mut offered = pin!(owner.offer());
            if pending_first {
                core::future::poll_fn(|cx| match offered.as_mut().poll(cx) {
                    Poll::Pending => Poll::Ready(()),
                    Poll::Ready(Ok(branch)) => panic!(
                        "an old retry cannot replace the incoming continuation with {}",
                        branch.label()
                    ),
                    Poll::Ready(Err(error)) => {
                        panic!("the live incoming continuation must remain available: {error:?}")
                    }
                })
                .await;
                client.send::<Following>(&()).await.unwrap();
            }
            crate::in_process::timeout(core::time::Duration::from_secs(1), async {
                let branch = offered.await.unwrap();
                assert_eq!(branch.label(), Following::LOGICAL_LABEL);
                branch.recv::<Following>().await.unwrap();
            })
            .await
            .expect("the queued continuation cannot wait for an old retry's resources");
        }
        first.send::<FirstWork>(&()).await.unwrap();
        owner.recv::<FirstWork>().await.unwrap();
        client.send::<First>(first).await.unwrap();
        drop(owner.recv::<First>().await.unwrap());
        owner.send::<LastPassed>(&()).await.unwrap();
        client.recv::<LastPassed>().await.unwrap();
        drop(second);
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn returning_a_conditionally_held_endpoint_preserves_its_selected_owner_guard() {
    type Notice = g::Msg<0, ()>;
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Proxy<'r> = g::Msg<2, Endpoint<'r, 2>>;
    type Work = g::Msg<3, ()>;
    type Returned<'r> = g::Msg<4, Endpoint<'r, 3>>;
    let global = g::seq(
        g::route(
            g::send::<0, 1, Notice>(),
            g::seq(g::send::<0, 1, Loan>(), g::send::<0, 1, Proxy>()),
        ),
        g::seq(g::send::<3, 0, Work>().roll(), g::send::<2, 0, Returned>()),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let proxy_program = project::<2>(&global);
    let resource_program = project::<3>(&global);

    for delegated in [false, true] {
        {
            let sid = SessionId::new(9170 + u32::from(delegated));
            // SAFETY: each case owns two distinct physical test resources and
            // keeps their sole bindings until every Endpoint has been dropped.
            let network = InMemoryTransport::new(unsafe {
                [
                    ((sid, 2), EndpointBinding::new()),
                    ((sid, 3), EndpointBinding::new()),
                ]
            });
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage.init().rendezvous(&mut slab, network).unwrap();
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut proxy = rv.enter(sid, &proxy_program).unwrap();
            let mut resource = rv.enter(sid, &resource_program).unwrap();

            if delegated {
                owner.send::<Loan>(resource).await.unwrap();
                owner.send::<Proxy>(proxy).await.unwrap();
                let mut resource = worker.recv::<Loan>().await.unwrap();
                let mut proxy = worker.recv::<Proxy>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
                proxy.send::<Returned>(resource).await.unwrap();
                let mut resource = owner.recv::<Returned>().await.unwrap();
                let mut attempted = pin!(resource.send::<Work>(&()));
                let mut cx = Context::from_waker(Waker::noop());
                let Poll::Ready(result) = attempted.as_mut().poll(&mut cx) else {
                    panic!("an inherited operation must be rejected rather than left pending");
                };
                let error = result
                    .expect_err("the other branch cannot grant this holder the rolled operation");
                assert!(format!("{error:?}").contains("OutOfOrder"), "{error:?}");
            } else {
                owner.send::<Notice>(&()).await.unwrap();
                worker.recv::<Notice>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
                proxy.send::<Returned>(resource).await.unwrap();
                let mut resource = owner.recv::<Returned>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
            }
        }
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn reentering_a_route_requires_the_views_used_by_its_selected_arm() {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Start = g::Msg<2, ()>;
    type Work = g::Msg<3, ()>;
    type Publish = g::Msg<4, ()>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::route(
            g::seq(g::send::<1, 0, Start>(), g::send::<3, 0, Work>()),
            g::send::<1, 0, Publish>(),
        )
        .roll(),
    );
    let service_program = project::<0>(&global);
    let client_program = project::<1>(&global);
    let file_program = project::<3>(&global);
    let sid = SessionId::new(9173);
    // SAFETY: this run issues one Endpoint for its one fixture resource.
    let network = InMemoryTransport::new(unsafe { [((sid, 3), EndpointBinding::new())] });
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, network).unwrap();
    let mut service = rv.enter(sid, &service_program).unwrap();
    let mut client = rv.enter(sid, &client_program).unwrap();
    let file = rv.enter(sid, &file_program).unwrap();
    service.send::<Loan>(file).await.unwrap();
    let mut file = client.recv::<Loan>().await.unwrap();
    client.send::<Start>(&()).await.unwrap();
    service.recv::<Start>().await.unwrap();
    file.send::<Work>(&()).await.unwrap();
    service.recv::<Work>().await.unwrap();
    drop(file);

    assert!(
        client
            .send::<Start>(&())
            .await
            .unwrap_err()
            .is_unavailable()
    );
    for _ in 0..2 {
        client.send::<Publish>(&()).await.unwrap();
        service.recv::<Publish>().await.unwrap();
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn moving_an_unrelated_resource_preserves_the_retained_roll() {
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type Work = g::Msg<1, ()>;
    type Proxy<'r> = g::Msg<2, Endpoint<'r, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 0, Work>().roll(), g::send::<0, 1, Proxy>()),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let proxy_program = project::<2>(&global);
    let resource_program = project::<3>(&global);

    {
        let sid = SessionId::new(9172);
        // SAFETY: two distinct resources with one binding each for this run.
        let network = InMemoryTransport::new(unsafe {
            [
                ((sid, 2), EndpointBinding::new()),
                ((sid, 3), EndpointBinding::new()),
            ]
        });
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage.init().rendezvous(&mut slab, network).unwrap();
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let proxy = rv.enter(sid, &proxy_program).unwrap();
        let resource = rv.enter(sid, &resource_program).unwrap();
        owner.send::<Loan>(resource).await.unwrap();
        let mut resource = worker.recv::<Loan>().await.unwrap();
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
        owner.send::<Proxy>(proxy).await.unwrap();
        let _proxy = worker.recv::<Proxy>().await.unwrap();
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn offer_does_not_publish_a_reentry_whose_resource_has_moved_to_the_continuation() {
    use core::cell::Cell;
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Work = g::Msg<2, ()>;
    type Passed = g::Msg<3, ()>;
    type Failed = g::Msg<4, ()>;
    type Following = g::Msg<5, ()>;
    type Alternative = g::Msg<6, ()>;
    let request = || g::seq(g::send::<3, 0, Work>(), g::send::<1, 0, Loan>());
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            request(),
            g::seq(
                g::route(
                    g::send::<0, 1, Passed>(),
                    g::seq(
                        g::seq(
                            g::send::<0, 1, Failed>(),
                            g::seq(g::send::<0, 1, Loan>(), request()),
                        )
                        .roll(),
                        g::send::<0, 1, Passed>(),
                    ),
                )
                .resolve::<9180>(),
                g::seq(
                    g::send::<0, 2, Loan>(),
                    g::seq(
                        g::route(g::send::<2, 0, Following>(), g::send::<2, 0, Alternative>()),
                        g::send::<2, 0, Loan>(),
                    ),
                ),
            ),
        ),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let receiver_program = project::<2>(&global);
    let resource_program = project::<3>(&global);
    for failures in [1, 2, 4] {
        let sid = SessionId::new(9180 + failures);
        // SAFETY: each case owns one fresh physical resource and its unique
        // binding across both runtimes until all endpoints have been dropped.
        let network = InMemoryTransport::new(unsafe { [((sid, 3), EndpointBinding::new())] });
        let actual = Cell::new(DecisionArm::Right);
        let mut owner_slab = [0; 64 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        owner_rv
            .set_resolver(
                &owner_program,
                ResolverRef::<9180>::decision_state(&actual, |value: &Cell<DecisionArm>| {
                    Ok(value.get())
                }),
            )
            .unwrap();
        let mut worker_slab = [0; 64 * 1024];
        let mut worker_storage = SessionKitStorage::uninit();
        let worker_rv = worker_storage
            .init()
            .rendezvous(&mut worker_slab, network)
            .unwrap();
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut receiver = worker_rv.enter(sid, &receiver_program).unwrap();
        let resource = owner_rv.enter(sid, &resource_program).unwrap();
        owner.send::<Loan>(resource).await.unwrap();
        let mut resource = worker.recv::<Loan>().await.unwrap();
        for _ in 0..failures {
            resource.send::<Work>(&()).await.unwrap();
            owner.recv::<Work>().await.unwrap();
            worker.send::<Loan>(resource).await.unwrap();
            let returned = owner.recv::<Loan>().await.unwrap();
            owner.send::<Failed>(&()).await.unwrap();
            worker.recv::<Failed>().await.unwrap();
            owner.send::<Loan>(returned).await.unwrap();
            resource = worker.recv::<Loan>().await.unwrap();
        }
        actual.set(DecisionArm::Left);
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
        worker.send::<Loan>(resource).await.unwrap();
        let returned = owner.recv::<Loan>().await.unwrap();
        owner.send::<Passed>(&()).await.unwrap();
        worker.recv::<Passed>().await.unwrap();
        owner.send::<Loan>(returned).await.unwrap();
        // Transport acceptance already moved the value out of this actor;
        // the receiver has deliberately not committed its receipt yet.
        let submitted = {
            let mut offered = pin!(owner.offer());
            core::future::poll_fn(|cx| match offered.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(()),
                Poll::Ready(Ok(branch)) => panic!(
                    "a transferred entry resource cannot authorize offered result {}",
                    branch.label()
                ),
                Poll::Ready(Err(error)) => {
                    panic!("the live continuation must remain available: {error:?}")
                }
            })
            .await;
            let submitted = receiver.recv::<Loan>().await.unwrap();
            if failures == 2 {
                receiver.send::<Alternative>(&()).await.unwrap();
                let branch = offered.await.unwrap();
                assert_eq!(branch.label(), Alternative::LOGICAL_LABEL);
                branch.recv::<Alternative>().await.unwrap();
            } else {
                receiver.send::<Following>(&()).await.unwrap();
                let branch = offered.await.unwrap();
                assert_eq!(branch.label(), Following::LOGICAL_LABEL);
                branch.recv::<Following>().await.unwrap();
            }
            submitted
        };
        receiver.send::<Loan>(submitted).await.unwrap();
        let returned = owner.recv::<Loan>().await.unwrap();
        // A real return restores the resource, so another inner visit is
        // legal. The enclosing resolver and continuation are not restarted.
        owner.send::<Failed>(&()).await.unwrap();
        worker.recv::<Failed>().await.unwrap();
        owner.send::<Loan>(returned).await.unwrap();
        let mut resource = worker.recv::<Loan>().await.unwrap();
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
        worker.send::<Loan>(resource).await.unwrap();
        let returned = owner.recv::<Loan>().await.unwrap();
        drop(returned);
    }
}
