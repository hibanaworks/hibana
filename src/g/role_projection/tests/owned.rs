use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn received_resource_endpoint_selects_its_own_resolved_route() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    type OtherRead = g::Msg<6, u32>;
    type Work = g::Resolve<
        g::Route<
            g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
            g::Seq<g::Send<10, 3, OtherRead>, g::Send<3, 10, Value>>,
        >,
        701,
    >;
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Seq<Work, g::Send<1, 0, Loan>>>;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 48 * 1024];
        let resolver_calls = core::cell::Cell::new(0usize);
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        rv.set_resolver(
            &worker_program,
            ResolverRef::<701>::decision_state(&resolver_calls, |calls| {
                calls.set(calls.get() + 1);
                Ok(DecisionArm::Left)
            }),
        )
        .unwrap();
        let sid = SessionId::new(827);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        assert!(futures::poll!(worker.offer()).is_pending());
        assert_eq!(resolver_calls.get(), 0);
        let branch = match futures::poll!(db.offer()) {
            core::task::Poll::Ready(result) => result.unwrap(),
            core::task::Poll::Pending => {
                panic!("a resource's enabled resolver must not wait for ingress")
            }
        };
        assert_eq!(branch.label(), 3);
        assert_eq!(resolver_calls.get(), 1);
        branch.send::<Read>(&42).await.unwrap();
        let branch = service.offer().await.unwrap();
        assert_eq!(branch.label(), 3);
        assert_eq!(branch.recv::<Read>().await.unwrap(), 42);
        service.send::<Value>(&43).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 43);
        worker.send::<Loan>(db).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    });
}

#[test]
fn two_owned_views_drive_independent_parallel_exchanges_with_the_same_message_type() {
    type Work = g::Par<
        g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
        g::Seq<g::Send<11, 4, Read>, g::Send<4, 11, Value>>,
    >;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<0, 1, OtherLoan>,
            g::Seq<Work, g::Seq<g::Send<1, 0, Loan>, g::Send<1, 0, OtherLoan>>>,
        >,
    >;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let db_service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let auth_service_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 64 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        let sid = SessionId::new(826);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let auth = rv.enter(sid, &auth_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut db_service = rv.enter(sid, &db_service_program).unwrap();
        let mut auth_service = rv.enter(sid, &auth_service_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        owner.send::<OtherLoan>(auth).await.unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        let mut auth = worker.recv::<OtherLoan>().await.unwrap();
        tokio::join!(
            // Poll the second source arm first. The message label/schema are
            // deliberately identical; logical view identity selects the step.
            async {
                auth.send::<Read>(&11).await.unwrap();
                assert_eq!(auth.recv::<Value>().await.unwrap(), 12);
            },
            async {
                db.send::<Read>(&7).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 8);
            },
            async {
                assert_eq!(db_service.recv::<Read>().await.unwrap(), 7);
                db_service.send::<Value>(&8).await.unwrap();
            },
            async {
                assert_eq!(auth_service.recv::<Read>().await.unwrap(), 11);
                auth_service.send::<Value>(&12).await.unwrap();
            }
        );
        worker.send::<Loan>(db).await.unwrap();
        worker.send::<OtherLoan>(auth).await.unwrap();
        let db = owner.recv::<Loan>().await.unwrap();
        let auth = owner.recv::<OtherLoan>().await.unwrap();
        drop(db);
        drop(auth);
    });
}

#[test]
fn two_received_resources_select_parallel_routes_without_serializing_their_views() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    type OtherRead = g::Msg<6, u32>;
    type Work = g::Par<
        g::Roll<
            g::Resolve<
                g::Route<
                    g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
                    g::Seq<g::Send<10, 3, OtherRead>, g::Send<3, 10, Value>>,
                >,
                701,
            >,
        >,
        g::Roll<
            g::Resolve<
                g::Route<
                    g::Seq<g::Send<11, 4, Read>, g::Send<4, 11, Value>>,
                    g::Seq<g::Send<11, 4, OtherRead>, g::Send<4, 11, Value>>,
                >,
                702,
            >,
        >,
    >;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<0, 1, OtherLoan>,
            g::Seq<Work, g::Seq<g::Send<1, 0, Loan>, g::Send<1, 0, OtherLoan>>>,
        >,
    >;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 64>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 64>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 64>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 64>::IMAGE_REF,
    );
    let db_service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 64>::IMAGE_REF,
    );
    let auth_service_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 64>::IMAGE_REF,
    );
    let image = &RoleProjection::<1, Steps, 64>::IMAGE_REF;
    assert_ne!(
        image.program().event_atom_at(2).lane,
        image.program().event_atom_at(6).lane
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 64 * 1024];
        let db_choice = core::cell::Cell::new(DecisionArm::Left);
        let auth_choice = core::cell::Cell::new(DecisionArm::Left);
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        rv.set_resolver(
            &worker_program,
            ResolverRef::<701>::decision_state(&db_choice, |arm| Ok(arm.get())),
        )
        .unwrap();
        rv.set_resolver(
            &worker_program,
            ResolverRef::<702>::decision_state(&auth_choice, |arm| Ok(arm.get())),
        )
        .unwrap();
        let sid = SessionId::new(828);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let auth = rv.enter(sid, &auth_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut db_service = rv.enter(sid, &db_service_program).unwrap();
        let mut auth_service = rv.enter(sid, &auth_service_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        owner.send::<OtherLoan>(auth).await.unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        let mut auth = worker.recv::<OtherLoan>().await.unwrap();
        tokio::join!(
            async {
                for (number, arm) in [(11, DecisionArm::Left), (13, DecisionArm::Right)] {
                    auth_choice.set(arm);
                    let branch = match futures::poll!(auth.offer()) {
                        core::task::Poll::Ready(result) => result.unwrap(),
                        core::task::Poll::Pending => {
                            panic!("the second resource route is independently enabled")
                        }
                    };
                    match branch.label() {
                        3 => branch.send::<Read>(&number).await.unwrap(),
                        6 => branch.send::<OtherRead>(&number).await.unwrap(),
                        label => panic!("unexpected branch {label}"),
                    }
                    assert_eq!(auth.recv::<Value>().await.unwrap(), number + 1);
                }
            },
            async {
                for (number, arm) in [(7, DecisionArm::Left), (9, DecisionArm::Right)] {
                    db_choice.set(arm);
                    let branch = match futures::poll!(db.offer()) {
                        core::task::Poll::Ready(result) => result.unwrap(),
                        core::task::Poll::Pending => {
                            panic!("the first resource route is independently enabled")
                        }
                    };
                    match branch.label() {
                        3 => branch.send::<Read>(&number).await.unwrap(),
                        6 => branch.send::<OtherRead>(&number).await.unwrap(),
                        label => panic!("unexpected branch {label}"),
                    }
                    assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                }
            },
            async {
                for expected in [7, 9] {
                    let branch = db_service.offer().await.unwrap();
                    let number = match branch.label() {
                        3 => branch.recv::<Read>().await.unwrap(),
                        6 => branch.recv::<OtherRead>().await.unwrap(),
                        label => panic!("unexpected branch {label}"),
                    };
                    assert_eq!(number, expected);
                    db_service.send::<Value>(&(number + 1)).await.unwrap();
                }
            },
            async {
                for expected in [11, 13] {
                    let branch = auth_service.offer().await.unwrap();
                    let number = match branch.label() {
                        3 => branch.recv::<Read>().await.unwrap(),
                        6 => branch.recv::<OtherRead>().await.unwrap(),
                        label => panic!("unexpected branch {label}"),
                    };
                    assert_eq!(number, expected);
                    auth_service.send::<Value>(&(number + 1)).await.unwrap();
                }
            }
        );
        worker.send::<Loan>(db).await.unwrap();
        worker.send::<OtherLoan>(auth).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
        drop(owner.recv::<OtherLoan>().await.unwrap());
    });
}

#[test]
fn rolled_owned_exchange_moves_the_returned_value_without_reattaching() {
    type Steps = g::Roll<DbLoan>;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 48 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        let sid = SessionId::new(825);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        tokio::join!(
            async {
                for _ in 0..2 {
                    owner.send::<Loan>(db).await.unwrap();
                    db = owner.recv::<Loan>().await.unwrap();
                    assert!(rv.enter(sid, &resource_program).is_err());
                }
                drop(db);
            },
            async {
                for number in [7, 9] {
                    let mut db = worker.recv::<Loan>().await.unwrap();
                    db.send::<Read>(&number).await.unwrap();
                    assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                    worker.send::<Loan>(db).await.unwrap();
                }
            },
            async {
                for number in [7, 9] {
                    assert_eq!(service.recv::<Read>().await.unwrap(), number);
                    service.send::<Value>(&(number + 1)).await.unwrap();
                }
            }
        );
    });
}

#[test]
fn owned_return_waits_for_resource_operations_in_the_same_actor_progress() {
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, DbLoan, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, DbLoan, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, DbLoan, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 32 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let network = TestTransport::new();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, network.clone()).unwrap();
        let sid = SessionId::new(822);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        assert!(worker.send::<Loan>(db).await.is_err());
        assert!(network.queue_is_empty());
        assert!(owner.recv::<Loan>().await.is_err());
        assert!(rv.enter(sid, &resource_program).is_err());
    });
}

#[test]
fn malformed_owned_frame_never_returns_an_endpoint_or_restores_the_sender() {
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, DbLoan, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, DbLoan, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, DbLoan, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 32 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let network = TestTransport::new();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, network.clone()).unwrap();
        let sid = SessionId::new(823);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        network.truncate_next_payload(1, 2);
        assert!(worker.recv::<Loan>().await.is_err());
        assert!(rv.enter(sid, &resource_program).is_err());
        assert!(owner.recv::<Loan>().await.is_err());
    });
}

#[test]
fn resolved_owned_branch_send_and_recv_preserve_the_resource_continuation() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    type AltLoan = g::Msg<5, Endpoint<'static, 10>>;
    type Steps = g::Seq<
        g::Resolve<g::Route<g::Send<0, 1, Loan>, g::Send<0, 1, AltLoan>>, 701>,
        g::Seq<g::Send<10, 3, Read>, g::Seq<g::Send<3, 10, Value>, g::Send<1, 0, Loan>>>,
    >;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 48 * 1024];
        let choose = DecisionArm::Left;
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        rv.set_resolver(
            &owner_program,
            ResolverRef::<701>::decision_state(&choose, |arm| Ok(*arm)),
        )
        .unwrap();
        let sid = SessionId::new(824);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        let branch = owner.offer().await.unwrap();
        assert_eq!(branch.label(), 1);
        branch.send::<Loan>(db).await.unwrap();
        let branch = worker.offer().await.unwrap();
        assert_eq!(branch.label(), 1);
        let mut db = branch.recv::<Loan>().await.unwrap();
        db.send::<Read>(&42).await.unwrap();
        assert_eq!(service.recv::<Read>().await.unwrap(), 42);
        service.send::<Value>(&43).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 43);
        worker.send::<Loan>(db).await.unwrap();
        let db = owner.recv::<Loan>().await.unwrap();
        drop(owner);
        drop(db);
    });
}
