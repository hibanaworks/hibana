use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn dropping_a_completed_owned_send_does_not_retire_the_returned_resource_view() {
    use crate::endpoint::proof_snapshot::view_tag;
    use core::{
        future::{Future, poll_fn},
        pin::Pin,
    };

    // AUTH is a second view at OWNER. Its later loan fixes that initial actor
    // in the global, so it can receive DB while OWNER's completed send future
    // still borrows the OWNER view and retains its stale DB payload value.
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<1, 11, Loan>,
            g::Seq<g::Send<10, 3, Read>, g::Seq<g::Send<3, 10, Value>, g::Send<0, 2, OtherLoan>>>,
        >,
    >;
    let () = crate::g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let receiver_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let network = TestTransport::new();
        let mut owner_slab = [0; 48 * 1024];
        let mut peer_slab = [0; 48 * 1024];
        let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut peer_storage = SessionKitStorage::<TestTransport>::uninit();
        let owner_kit = owner_storage.init();
        let peer_kit = peer_storage.init();
        let owner_rv = owner_kit
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let peer_rv = peer_kit.rendezvous(&mut peer_slab, network).unwrap();
        let sid = SessionId::new(873);
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let mut auth = owner_rv.enter(sid, &auth_program).unwrap();
        let db = owner_rv.enter(sid, &db_program).unwrap();
        let mut worker = peer_rv.enter(sid, &worker_program).unwrap();
        let mut service = peer_rv.enter(sid, &service_program).unwrap();
        let mut receiver = peer_rv.enter(sid, &receiver_program).unwrap();

        let old_tag = view_tag(&db);
        // SAFETY: owner_storage retains the observed pinned view record.
        assert!(unsafe { old_tag.matches_header() });
        let mut loan = owner.send::<Loan>(db);
        poll_fn(|cx| Pin::new(&mut loan).poll(cx)).await.unwrap();
        // SAFETY: successful transfer retains the original runtime storage.
        assert!(!unsafe { old_tag.matches_header() });
        let db = worker.recv::<Loan>().await.unwrap();
        worker.send::<Loan>(db).await.unwrap();
        let mut db = auth.recv::<Loan>().await.unwrap();
        // SAFETY: this return reuses the same pinned record, with a fresh tag.
        assert!(!unsafe { old_tag.matches_header() });
        // The old send has completed, but its owned payload is only destroyed
        // here, after DB has been reissued at the original physical actor.
        drop(loan);
        let (value, ()) = tokio::join!(
            async {
                db.send::<Read>(&42).await.unwrap();
                db.recv::<Value>().await.unwrap()
            },
            async {
                let key = service.recv::<Read>().await.unwrap();
                service.send::<Value>(&(key + 1)).await.unwrap();
            },
        );
        assert_eq!(value, 43);
        owner.send::<OtherLoan>(auth).await.unwrap();
        drop(receiver.recv::<OtherLoan>().await.unwrap());
    });
}

#[test]
fn a_message_between_co_resident_views_consumes_both_local_halves_before_handoff() {
    type Steps = g::Seq<g::Send<0, 10, Read>, g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>>;
    let () = crate::g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 48 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        let sid = SessionId::new(846);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &db_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();

        // OWNER and DB are different logical roles held by the same actor.
        // Sending must leave DB's matching receive available in that actor.
        owner.send::<Read>(&42).await.unwrap();
        assert_eq!(db.recv::<Read>().await.unwrap(), 42);
        owner.send::<Loan>(db).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        worker.send::<Loan>(db).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    });
}

#[test]
fn one_resource_binding_cannot_issue_again_in_another_runtime_or_session() {
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>;
    let () = crate::g::ProgramProjection::<Steps, 8>::VALIDATION;
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let network = TestTransport::new();
    let mut other_slab = [0u8; 32 * 1024];
    let mut other_storage = SessionKitStorage::<TestTransport>::uninit();
    let other_kit = other_storage.init();
    let other_rv = other_kit
        .rendezvous(&mut other_slab, network.clone())
        .unwrap();
    {
        let mut owner_slab = [0u8; 32 * 1024];
        let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
        let owner_kit = owner_storage.init();
        let owner_rv = owner_kit
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let db = owner_rv
            .enter(SessionId::new(840), &resource_program)
            .unwrap();
        for sid in [840, 841] {
            let error = other_rv
                .enter(SessionId::new(sid), &resource_program)
                .err()
                .expect("a new runtime/session cannot duplicate the physical right");
            assert_eq!(
                error,
                crate::runtime::AttachError::endpoint_binding_unavailable(10)
                    .with_operation(crate::session::cluster::error::AttachOp::Enter)
            );
        }
        drop(db);
    }
    // Neither dropping an unfinished Endpoint nor destroying its whole kit
    // fences a possibly remote right. The resource authority stays consumed.
    let error = other_rv
        .enter(SessionId::new(842), &resource_program)
        .err()
        .expect("runtime destruction must not resurrect the initial right");
    assert_eq!(
        error,
        crate::runtime::AttachError::endpoint_binding_unavailable(10)
            .with_operation(crate::session::cluster::error::AttachOp::Enter)
    );
}

#[test]
fn failed_initial_attachment_does_not_consume_the_physical_resource_binding() {
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>;
    let () = crate::g::ProgramProjection::<Steps, 8>::VALIDATION;
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let network = TestTransport::new();
    {
        let mut slab = [0u8; 2048];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, network.clone()).unwrap();
        assert!(rv.enter(SessionId::new(843), &resource_program).is_err());
    }
    let mut slab = [0u8; 32 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rv = kit.rendezvous(&mut slab, network).unwrap();
    // Same identity and resource, after a failure before publication.
    let db = rv.enter(SessionId::new(843), &resource_program).unwrap();
    drop(db);
}

#[test]
fn received_resource_keeps_working_after_its_original_runtime_is_destroyed() {
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    let () = crate::g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let network = TestTransport::new();
        let mut worker_slab = [0u8; 32 * 1024];
        let mut service_slab = [0u8; 32 * 1024];
        let mut worker_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut service_storage = SessionKitStorage::<TestTransport>::uninit();
        let worker_kit = worker_storage.init();
        let service_kit = service_storage.init();
        let worker_rv = worker_kit
            .rendezvous(&mut worker_slab, network.clone())
            .unwrap();
        let service_rv = service_kit
            .rendezvous(&mut service_slab, network.clone())
            .unwrap();
        let sid = SessionId::new(844);
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        let mut db = {
            let mut owner_slab = [0u8; 32 * 1024];
            let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
            let owner_kit = owner_storage.init();
            let owner_rv = owner_kit
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            owner.send::<Loan>(db).await.unwrap();
            worker.recv::<Loan>().await.unwrap()
        };
        // The sender's kit/slab/ports have all been destroyed. The received
        // value owns a continuation in the receiver, not a pointer into them.
        assert!(worker_rv.enter(SessionId::new(845), &db_program).is_err());
        tokio::join!(
            async {
                db.send::<Read>(&42).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 43);
            },
            async {
                let key = service.recv::<Read>().await.unwrap();
                service.send::<Value>(&(key + 1)).await.unwrap();
            }
        );
    });
}

#[test]
fn physical_service_grants_the_initial_right_and_remote_holders_only_receive_it() {
    type Steps = g::Seq<
        g::Send<3, 0, Loan>,
        g::Seq<
            g::Send<0, 1, Loan>,
            g::Seq<
                g::Send<10, 3, Read>,
                g::Seq<g::Send<3, 10, Value>, g::Seq<g::Send<1, 0, Loan>, g::Send<0, 3, Loan>>>,
            >,
        >,
    >;
    let () = crate::g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let network = TestTransport::new();
        // The two remote holders can carry frames but have no initial issuer.
        // All initial authority stays at the service's physical resource owner.
        let mut owner_transport = network.clone();
        owner_transport.bindings = None;
        let mut worker_transport = network.clone();
        worker_transport.bindings = None;
        let mut owner_slab = [0u8; 32 * 1024];
        let mut worker_slab = [0u8; 32 * 1024];
        let mut service_slab = [0u8; 32 * 1024];
        let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut worker_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut service_storage = SessionKitStorage::<TestTransport>::uninit();
        let owner_kit = owner_storage.init();
        let worker_kit = worker_storage.init();
        let service_kit = service_storage.init();
        let owner_rv = owner_kit
            .rendezvous(&mut owner_slab, owner_transport)
            .unwrap();
        let worker_rv = worker_kit
            .rendezvous(&mut worker_slab, worker_transport)
            .unwrap();
        let service_rv = service_kit.rendezvous(&mut service_slab, network).unwrap();
        let sid = SessionId::new(846);
        for result in [
            owner_rv.enter(sid, &db_program),
            worker_rv.enter(sid, &db_program),
        ] {
            assert_eq!(
                result
                    .err()
                    .expect("a carrier does not imply an initial resource right"),
                crate::runtime::AttachError::endpoint_binding_unavailable(10)
                    .with_operation(crate::session::cluster::error::AttachOp::Enter)
            );
        }
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        let db = service_rv.enter(sid, &db_program).unwrap();
        tokio::join!(
            async {
                service.send::<Loan>(db).await.unwrap();
                let key = service.recv::<Read>().await.unwrap();
                service.send::<Value>(&(key + 1)).await.unwrap();
                drop(service.recv::<Loan>().await.unwrap());
            },
            async {
                let db = owner.recv::<Loan>().await.unwrap();
                owner.send::<Loan>(db).await.unwrap();
                let db = owner.recv::<Loan>().await.unwrap();
                owner.send::<Loan>(db).await.unwrap();
            },
            async {
                let mut db = worker.recv::<Loan>().await.unwrap();
                db.send::<Read>(&42).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 43);
                worker.send::<Loan>(db).await.unwrap();
            }
        );
    });
}

#[test]
fn typed_projection_checks_the_holders_knowledge_before_delegated_work() {
    type OtherDbLoan = g::Msg<2, Endpoint<'static, 10>>;
    type OtherRead = g::Msg<6, u32>;
    let global = g::route(
        g::seq(g::send::<0, 1, Loan>(), g::send::<10, 3, Read>()),
        g::seq(
            g::send::<0, 1, OtherDbLoan>(),
            g::send::<10, 3, OtherRead>(),
        ),
    );
    assert!(g::diagnose(&global).is_none());
}

#[test]
fn typed_projection_rejects_equal_candidate_sets_with_different_actual_participants() {
    let global = g::seq(
        g::route(
            g::seq(g::send::<0, 1, Loan>(), g::send::<0, 2, OtherLoan>()),
            g::seq(g::send::<0, 2, Loan>(), g::send::<0, 1, OtherLoan>()),
        )
        .resolve::<701>(),
        g::seq(
            g::route(
                g::send::<0, 10, g::Msg<5, ()>>(),
                g::send::<0, 11, g::Msg<6, ()>>(),
            ),
            g::send::<1, 3, Read>(),
        ),
    );
    let diagnostic = g::diagnose(&global).expect("the actual holder can miss this branch choice");
    assert_eq!(diagnostic.problem, g::ProjectionProblem::UnprojectableRoute);
}

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
#[test]
fn a_conditional_holder_keeps_resource_rolls_and_returns_through_an_owned_carrier() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    type Notice = g::Msg<8, ()>;
    type AfterRead = g::Msg<9, u32>;
    type Choice = g::Resolve<
        g::Route<
            g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<0, 1, OtherLoan>, g::Send<0, 2, Notice>>>,
            g::Seq<g::Send<0, 2, Loan>, g::Seq<g::Send<0, 2, OtherLoan>, g::Send<0, 1, Notice>>>,
        >,
        701,
    >;
    type Work = g::Roll<g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    type Steps = g::Seq<
        Choice,
        g::Seq<
            Work,
            g::Seq<g::Send<11, 0, Loan>, g::Seq<g::Send<10, 3, AfterRead>, g::Send<3, 10, Value>>>,
        >,
    >;
    let () = crate::g::ProgramProjection::<Steps, 64>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 64>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 64>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 64>::IMAGE_REF,
    );
    let a_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 64>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 64>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 64>::IMAGE_REF,
    );
    for choose in [DecisionArm::Left, DecisionArm::Right] {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut owner_slab = [0u8; 32 * 1024];
            let mut a_slab = [0u8; 32 * 1024];
            let mut b_slab = [0u8; 32 * 1024];
            let mut service_slab = [0u8; 32 * 1024];
            let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
            let mut a_storage = SessionKitStorage::<TestTransport>::uninit();
            let mut b_storage = SessionKitStorage::<TestTransport>::uninit();
            let mut service_storage = SessionKitStorage::<TestTransport>::uninit();
            let network = TestTransport::new();
            let owner_kit = owner_storage.init();
            let a_kit = a_storage.init();
            let b_kit = b_storage.init();
            let service_kit = service_storage.init();
            let rv = owner_kit
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let a_rv = a_kit.rendezvous(&mut a_slab, network.clone()).unwrap();
            let b_rv = b_kit.rendezvous(&mut b_slab, network.clone()).unwrap();
            let service_rv = service_kit.rendezvous(&mut service_slab, network).unwrap();
            rv.set_resolver(
                &owner_program,
                ResolverRef::<701>::decision_state(&choose, |arm| Ok(*arm)),
            )
            .unwrap();
            let sid = SessionId::new(830);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &db_program).unwrap();
            let auth = rv.enter(sid, &auth_program).unwrap();
            let mut a = a_rv.enter(sid, &a_program).unwrap();
            let mut b = b_rv.enter(sid, &b_program).unwrap();
            let mut service = service_rv.enter(sid, &service_program).unwrap();
            tokio::join!(
                async {
                    let branch = owner.offer().await.unwrap();
                    branch.send::<Loan>(db).await.unwrap();
                    owner.send::<OtherLoan>(auth).await.unwrap();
                    owner.send::<Notice>(&()).await.unwrap();
                    let mut db = owner.recv::<Loan>().await.unwrap();
                    drop(owner);
                    assert!(rv.enter(sid, &db_program).is_err());
                    db.send::<AfterRead>(&47).await.unwrap();
                    assert_eq!(db.recv::<Value>().await.unwrap(), 48);
                },
                async {
                    let branch = a.offer().await.unwrap();
                    match branch.label() {
                        1 => {
                            let mut db = branch.recv::<Loan>().await.unwrap();
                            let mut auth = a.recv::<OtherLoan>().await.unwrap();
                            for number in [41, 43] {
                                db.send::<Read>(&number).await.unwrap();
                                assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                            }
                            auth.send::<Loan>(db).await.unwrap();
                        }
                        8 => {
                            branch.recv::<Notice>().await.unwrap();
                        }
                        label => panic!("unexpected choice {label}"),
                    }
                },
                async {
                    let branch = b.offer().await.unwrap();
                    match branch.label() {
                        1 => {
                            let mut db = branch.recv::<Loan>().await.unwrap();
                            let mut auth = b.recv::<OtherLoan>().await.unwrap();
                            for number in [41, 43] {
                                db.send::<Read>(&number).await.unwrap();
                                assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                            }
                            auth.send::<Loan>(db).await.unwrap();
                        }
                        8 => {
                            branch.recv::<Notice>().await.unwrap();
                        }
                        label => panic!("unexpected choice {label}"),
                    }
                },
                async {
                    for expected in [41, 43] {
                        assert_eq!(service.recv::<Read>().await.unwrap(), expected);
                        service.send::<Value>(&(expected + 1)).await.unwrap();
                    }
                    assert_eq!(service.recv::<AfterRead>().await.unwrap(), 47);
                    service.send::<Value>(&48).await.unwrap();
                },
            );
        });
    }
}
