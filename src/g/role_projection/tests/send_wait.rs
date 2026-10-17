//! A logical view may await a different view's projected predecessor.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Wake,
};

struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

type Global = g::Seq<
    g::Send<0, 2, Read>,
    g::Seq<g::Send<10, 3, Value>, g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>>,
>;

#[test]
fn a_send_waits_for_another_views_predecessor_without_sending_early() {
    let () = g::ProgramProjection::<Global, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 8>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 8>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = std::vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        let sid = SessionId::new(849);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &db_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut reader = rv.enter(sid, &reader_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        // Abandoning preparation must not touch transport or lose the step.
        {
            let mut cancelled = pin!(db.send::<Value>(&99));
            assert!(
                cancelled
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
        }
        let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        {
            let mut send = pin!(db.send::<Value>(&43));
            let poll = send.as_mut().poll(&mut cx);
            assert!(poll.is_pending(), "the other view must run first: {poll:?}");
            let mut recv = pin!(service.recv::<Value>());
            assert!(recv.as_mut().poll(&mut cx).is_pending());
            let before = wakes.0.load(Ordering::Relaxed);
            owner.send::<Read>(&42).await.unwrap();
            assert!(
                wakes.0.load(Ordering::Relaxed) > before,
                "the actor commit must wake the waiting view"
            );
            send.await.unwrap();
            assert_eq!(recv.await.unwrap(), 43);
        }
        assert_eq!(reader.recv::<Read>().await.unwrap(), 42);
        owner.send::<Loan>(db).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        worker.send::<Loan>(db).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    });
}

#[test]
fn an_owned_send_rejects_a_predecessor_that_needs_its_payload_view() {
    let () = g::ProgramProjection::<DbLoan, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, DbLoan, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, DbLoan, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, DbLoan, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = std::vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        let sid = SessionId::new(850);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        // DB still has Read/Value work. This future owns DB, so that work
        // cannot run elsewhere and waiting would hide a localside mistake.
        let mut early = pin!(worker.send::<Loan>(db));
        let poll = early.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        assert!(
            matches!(poll, core::task::Poll::Ready(Err(_))),
            "an early return must fail: {poll:?}"
        );
    });
}

#[test]
fn an_owned_send_can_wait_for_a_distinct_live_view() {
    type Steps = g::Seq<
        g::Send<11, 3, Read>,
        g::Seq<
            g::Send<0, 1, Loan>,
            g::Seq<g::Send<0, 1, OtherLoan>, g::Seq<g::Send<1, 0, Loan>, g::Send<1, 0, OtherLoan>>>,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 8>::IMAGE_REF,
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
        let mut slab = std::vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        let sid = SessionId::new(851);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        tokio::join!(biased;
            async { owner.send::<Loan>(db).await.unwrap(); },
            async { auth.send::<Read>(&17).await.unwrap(); },
        );
        assert_eq!(service.recv::<Read>().await.unwrap(), 17);
        owner.send::<OtherLoan>(auth).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        let auth = worker.recv::<OtherLoan>().await.unwrap();
        worker.send::<Loan>(db).await.unwrap();
        worker.send::<OtherLoan>(auth).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
        drop(owner.recv::<OtherLoan>().await.unwrap());
    });
}

#[test]
fn a_waiting_send_does_not_run_its_resolver_before_the_predecessor() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    type Other = g::Msg<6, u32>;
    type Steps = g::Seq<
        g::Send<0, 2, Value>,
        g::Seq<
            g::Resolve<g::Route<g::Send<10, 3, Read>, g::Send<10, 3, Other>>, 703>,
            g::Seq<g::Send<0, 1, Loan>, g::Send<1, 0, Loan>>,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let calls = core::cell::Cell::new(0usize);
        let mut slab = std::vec![0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        rv.set_resolver(
            &owner_program,
            ResolverRef::<703>::decision_state(&calls, |calls| {
                calls.set(calls.get() + 1);
                Ok(DecisionArm::Left)
            }),
        )
        .unwrap();
        let sid = SessionId::new(852);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &db_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut reader = rv.enter(sid, &reader_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        {
            let mut send = pin!(db.send::<Read>(&21));
            assert!(
                send.as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            assert_eq!(calls.get(), 0);
            owner.send::<Value>(&20).await.unwrap();
            send.await.unwrap();
            assert!(calls.get() > 0);
        }
        assert_eq!(service.recv::<Read>().await.unwrap(), 21);
        assert_eq!(reader.recv::<Value>().await.unwrap(), 20);
        owner.send::<Loan>(db).await.unwrap();
        let db = worker.recv::<Loan>().await.unwrap();
        worker.send::<Loan>(db).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    });
}

#[test]
fn dropping_an_owned_send_cancels_the_run_before_or_during_preparation() {
    use crate::endpoint::proof_snapshot::{actor_fault, actor_snapshot, view_tag};
    use crate::rendezvous::SessionFaultKind;
    type Steps = g::Seq<
        g::Send<11, 3, Read>,
        g::Seq<
            g::Send<0, 1, Loan>,
            g::Seq<g::Send<0, 1, OtherLoan>, g::Seq<g::Send<1, 0, Loan>, g::Send<1, 0, OtherLoan>>>,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 8>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        #[cfg(not(miri))]
        let mut retirement_observations = std::vec::Vec::new();
        // Test stimuli, not application progress: unpolled, waiting for the
        // other view, and prepared while the transport is paused.
        for (name, poll_before_drop, finish_predecessor) in [
            ("unpolled", false, false),
            ("predecessor", true, false),
            ("transport", true, true),
        ] {
            let network = TestTransport::new();
            let mut slab = std::vec![0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(853);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut service = rv.enter(sid, &service_program).unwrap();
            if finish_predecessor {
                auth.send::<Read>(&17).await.unwrap();
                assert_eq!(service.recv::<Read>().await.unwrap(), 17);
                network.pause_send(0);
            }
            let tag = view_tag(&db);
            let (before, before_fault) = {
                let mut send = pin!(owner.send::<Loan>(db));
                if poll_before_drop {
                    assert!(
                        send.as_mut()
                            .poll(&mut Context::from_waker(Waker::noop()))
                            .is_pending()
                    );
                }
                // SAFETY: AUTH is distinct from the future's borrowed carrier
                // and payload; the poll released its actor mutation lease.
                (unsafe { actor_snapshot(&auth) }, unsafe {
                    actor_fault(&auth)
                })
            };
            assert_eq!(before_fault, None, "{name}");
            // SAFETY: AUTH still owns its distinct view in the original actor.
            let after = unsafe { actor_snapshot(&auth) };
            assert_eq!(before.steps, after.steps);
            assert_eq!(before.choices, after.choices);
            assert_eq!(before.counter, after.counter);
            assert_eq!(before.live_views, after.live_views + 1);
            // SAFETY: the original DB header remains allocated by storage.
            assert!(!unsafe { tag.matches_header() });
            // SAFETY: AUTH retains the same TestTransport actor and registry.
            let after_fault = unsafe { actor_fault(&auth) };
            assert_eq!(
                after_fault,
                Some(SessionFaultKind::EndpointDropped),
                "{name}"
            );
            assert!(network.queue_is_empty());
            assert!(rv.enter(sid, &db_program).is_err());
            if finish_predecessor {
                network.resume_send();
            }
            assert!(auth.send::<Read>(&17).await.is_err());
            // SAFETY: the failed operation cannot replace the first fault.
            assert_eq!(
                unsafe { actor_fault(&auth) },
                Some(SessionFaultKind::EndpointDropped)
            );
            // SAFETY: observing a fault does not destroy AUTH's pinned actor.
            let rejected = unsafe { actor_snapshot(&auth) };
            assert_eq!(after.steps, rejected.steps);
            assert_eq!(after.choices, rejected.choices);
            assert_eq!(after.counter, rejected.counter);
            assert_eq!(after.live_views, rejected.live_views);
            assert!(after.view_headers().eq(rejected.view_headers()));
            assert!(network.queue_is_empty());
            #[cfg(not(miri))]
            retirement_observations.push(super::retirement::RetirementSample {
                name,
                before,
                after,
                before_fault,
                after_fault,
                origin: super::retirement::RetirementOrigin::Future,
                complete: false,
            });
        }
        #[cfg(not(miri))]
        {
            use crate::test_support::{
                lean_choreo::LeanChoreo, lean_descriptor::descriptor_source,
            };
            super::retirement::write_retirements(
                "OwnedDropGenerated",
                Steps::lean_source(),
                descriptor_source(
                    crate::global::compiled::images::RoleDescriptorRef::from_resident(
                        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
                    ),
                ),
                &retirement_observations,
            );
        }
    });
}
