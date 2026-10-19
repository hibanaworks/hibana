//! Readiness suspension must not freeze ownership or source admission facts.
//! Every operation below is a real Endpoint operation on the same actor.
use super::*;
use crate::{
    endpoint::proof_snapshot::{actor_snapshot, view_tag},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};

type Tick = g::Msg<39, ()>;
type Steps = g::Seq<
    g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
    g::Seq<g::Send<11, 1, Loan>, g::Send<0, 2, OtherLoan>>,
>;
type Independent = g::Msg<37, u32>;
type Global = g::Par<Steps, g::Roll<g::Send<0, 8, Independent>>>;

#[test]
fn a_pending_body_restart_cannot_publish_after_another_view_hands_its_resource_away() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time().start_paused(true).build().unwrap().block_on(async {
        let network = TestTransport::new();
        let mut slab = [0u8; 64 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let rv = storage.init().rendezvous(&mut slab, network.clone()).unwrap();
        let sid = SessionId::new(9030);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut db = rv.enter(sid, &db_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut ticker = rv.enter(sid, &ticker_program).unwrap();
        let mut reader = rv.enter(sid, &reader_program).unwrap();

        owner.send::<Tick>(&()).await.unwrap();
        db.send::<Read>(&7).await.unwrap();
        ticker.recv::<Tick>().await.unwrap();
        assert_eq!(reader.recv::<Read>().await.unwrap(), 7);

        network.pause_send(0);
        let mut pending = pin!(owner.send::<Tick>(&()));
        poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        }).await;
        auth.send::<Loan>(db).await.unwrap();
        let transferred = worker.recv::<Loan>().await.unwrap();
        assert!(network.queue_is_empty());
        // SAFETY: auth retains the same actor created with TestTransport.
        let before = unsafe { actor_snapshot(&auth) };
        network.resume_send();
        let outcome = tokio::time::timeout(core::time::Duration::from_secs(10), pending).await;
        // SAFETY: the independently borrowed AUTH view still retains that actor.
        let after = unsafe { actor_snapshot(&auth) };
        assert!(matches!(outcome, Ok(Err(_))),
            "a stale reentry plan sent after DB left: {outcome:?}, before={before:?}, after={after:?}");
        assert!(network.queue_is_empty(), "stale Tick reached the peer queue");
        assert_eq!(before.counter, after.counter);
        assert!(before.steps.iter().map(|step| step.done)
            .eq(after.steps.iter().map(|step| step.done)));
        drop(transferred);
    });
}

#[test]
fn a_pending_send_waits_for_a_fresh_other_view_predecessor_and_then_finishes() {
    type Global = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<g::Send<11, 1, Value>, g::Seq<g::Send<0, 2, Loan>, g::Send<0, 2, OtherLoan>>>,
    >;
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9031);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            network.pause_send(11);
            let mut pending = pin!(auth.send::<Value>(&777));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            owner.send::<Tick>(&()).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            network.resume_send();
            poll_fn(|cx| {
                assert!(
                    pending.as_mut().poll(cx).is_pending(),
                    "fresh DB work must still precede the staged tail"
                );
                Poll::Ready(())
            })
            .await;
            assert!(network.queue_is_empty());
            db.send::<Read>(&9).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 9);
            tokio::time::timeout(core::time::Duration::from_secs(10), pending)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(worker.recv::<Value>().await.unwrap(), 777);
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_pending_owned_send_keeps_its_payload_unavailable_for_resource_dependent_reentry() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let independent_program = crate::global::role_program::role_program_from_image::<8>(
        &RoleProjection::<8, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9032);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut independent = rv.enter(sid, &independent_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            network.pause_send(11);
            let payload_tag = view_tag(&db);
            let mut pending = pin!(auth.send::<Loan>(db));
            poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            // The send future owns DB. Even before transport acceptance, that
            // value cannot perform the next visit's resource action.
            // Independent par work on this same actor must remain enabled.
            // SAFETY: DB's pinned TestTransport actor is retained by auth;
            // this observation runs between polls with no kernel lease held.
            assert!(unsafe { payload_tag.is_send_payload::<TestTransport>() });
            // SAFETY: owner remains attached to this TestTransport actor.
            let pending_snapshot = unsafe { view_tag(&owner).snapshot::<TestTransport>() };
            let borrowed_db = pending_snapshot.views.iter().find(|view| view.role == 10).unwrap();
            assert_eq!(borrowed_db.run, pending_snapshot.run);
            assert_eq!(borrowed_db.operation, "SendPayload");
            let mut independent_observations = std::vec::Vec::new();
            for value in [2, 3] {
                // SAFETY: the ordinary owner view is not borrowed by pending.
                let before = unsafe { actor_snapshot(&owner) };
                owner.send::<Independent>(&value).await.unwrap();
                // SAFETY: the completed send released the same live owner view.
                let after = unsafe { actor_snapshot(&owner) };
                assert!(before.view_headers().eq(after.view_headers()));
                assert_eq!(after.views.iter().find(|view| view.role == 10).unwrap().operation, "SendPayload");
                independent_observations.push((before, after));
                assert_eq!(independent.recv::<Independent>().await.unwrap(), value);
            }
            let mut restart = pin!(owner.send::<Tick>(&()));
            poll_fn(|cx| {
                assert!(
                    restart.as_mut().poll(cx).is_pending(),
                    "a fresh body started while its resource belonged to an owned send future"
                );
                Poll::Ready(())
            })
            .await;
            assert!(
                network.queue_is_empty(),
                "resource-dependent work escaped before its capability was available"
            );
            network.resume_send();
            tokio::time::timeout(core::time::Duration::from_secs(10), pending)
                .await
                .unwrap()
                .unwrap();
            let transferred = worker.recv::<Loan>().await.unwrap();
            let result = tokio::time::timeout(core::time::Duration::from_secs(10), restart).await;
            assert!(
                matches!(result, Ok(Err(_))),
                "the resource permanently left: {result:?}"
            );
            assert!(network.queue_is_empty());
            drop(transferred);
            #[cfg(not(miri))]
            {
                use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
                let tick = pending_snapshot.steps.iter().position(|row|
                    row.view == 0 && matches!(row.action, LocalAction::Send { label: 39, .. })
                ).unwrap();
                let tick_scope = pending_snapshot.steps[tick].reentry_scope.unwrap();
                let loan = pending_snapshot.steps.iter().position(|row|
                    row.view == 11 && matches!(row.action, LocalAction::Send { label: 1, .. })
                ).unwrap();
                let independent_event = pending_snapshot.steps.iter().position(|row|
                    row.view == 0 && matches!(row.action, LocalAction::Send { label: 37, .. })
                ).unwrap();
                let independent_scope = independent_observations[1].0.steps[independent_event].reentry_scope.unwrap();
                let mut source = std::format!(
                    "import Hibana.ActorReentryCommit\n\nset_option maxRecDepth 10000\n\n\
                     def pendingBorrowChoreo : Hibana.Choreo := {}\n\n\
                     def pendingBorrowImage : Hibana.RustDescriptorImage := {}\n\n\
                     def pendingBorrowBefore : Hibana.ActorProgressObservation := {}\n\n\
                     theorem pendingBorrowSnapshotValid : pendingBorrowBefore.valid = true := by decide\n\n\
                     theorem pendingBorrowDBIssued : pendingBorrowBefore.viewIssued 10 = true := by decide\n\n\
                     theorem pendingBorrowDBHeldBySend :\n  \
                     (Hibana.observedActorView? pendingBorrowBefore 10).map Hibana.ActorViewObservation.operation = some .sendPayload := by decide\n\n\
                     theorem pendingBorrowHeadersMatch :\n  \
                     Hibana.observedRollEntryHeadersMatch pendingBorrowBefore pendingBorrowImage {tick_scope} = true := by decide\n\n\
                     theorem pendingBorrowRestartWaits :\n  \
                     Hibana.observedRollEntryBorrowMatches pendingBorrowBefore pendingBorrowImage {tick_scope} {tick} 0 = false := by decide\n\n\
                     theorem pendingBorrowOwnPayloadFromSource :\n  \
                     Hibana.observedOutgoingPayload? pendingBorrowImage {loan} 11 = some (some 10) := by decide\n\n\
                     theorem pendingBorrowOwnSendCanFinish :\n  \
                     Hibana.observedRollEntryBorrowMatches pendingBorrowBefore pendingBorrowImage {tick_scope} {loan} 11 = true := by decide\n\n",
                    Global::lean_source(),
                    descriptor_source(RoleDescriptorRef::from_resident(&RoleProjection::<0, Global, 16>::IMAGE_REF)),
                    super::commit::observation_source(&pending_snapshot),
                );
                for (visit, (before, after)) in independent_observations.iter().enumerate() {
                    let sample = std::format!("pendingIndependent{visit}");
                    let ty = if visit == 0 { "ActorCommitObservation" } else { "ActorReentryCommitObservation" };
                    let path = if visit == 0 { std::string::String::from("pathIndex := 0") } else {
                        std::format!("previousPath := 0, nextPath := 0, scope := {independent_scope}")
                    };
                    let proof = if visit == 0 { "accepted_actor_commit_matches_source_progress" } else { "accepted_body_reentry_restores_source_progress" };
                    source.push_str(&std::format!(
                        "def {sample} : Hibana.{ty} := {{\n  \
                         {path}, event := {independent_event}, view := 0,\n  \
                         before := {},\n  after := {}\n}}\n\n\
                         theorem {sample}Accepted : {sample}.check pendingBorrowImage pendingBorrowChoreo = true := by decide\n\n\
                         theorem {sample}SourceOrdered : {sample}.SourceOrdered pendingBorrowImage pendingBorrowChoreo :=\n  \
                         Hibana.{proof} {sample}Accepted\n\n",
                        super::commit::observation_source(before), super::commit::observation_source(after),
                    ));
                }
                let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&output).unwrap();
                std::fs::write(output.join("PendingBorrowGenerated.lean"), source).unwrap();
            }
        });
}

#[test]
fn an_unpolled_owned_send_cannot_wait_on_its_payload_after_another_view_restarts() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let independent_program = crate::global::role_program::role_program_from_image::<8>(
        &RoleProjection::<8, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9041);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut independent = rv.enter(sid, &independent_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);

            use crate::endpoint::proof_snapshot::owned_future::{Capture, OwnedFutureBoundary};
            let auth_tag = view_tag(&auth);
            let db_tag = view_tag(&db);
            // SAFETY: both values retain this pinned TestTransport actor.
            let (carrier_id, payload_id, before) = unsafe {
                (
                    auth_tag.value_identity(),
                    db_tag.value_identity(),
                    auth_tag.snapshot::<TestTransport>(),
                )
            };
            let capture = Capture::start();
            {
                let mut pending = pin!(auth.send::<Loan>(db));
                let sealed = capture.snapshot();
                let [
                    OwnedFutureBoundary::Seal {
                        carrier,
                        payload,
                        label,
                        region,
                    },
                ] = sealed.as_slice()
                else {
                    panic!("the actual constructor was not observed: {sealed:?}");
                };
                assert_eq!(*carrier, carrier_id);
                assert_eq!(*payload, payload_id);
                assert_eq!(*label, 1);
                assert!(region.is_whole());
                // SAFETY: the future retains the same pinned actor between polls.
                assert_eq!(unsafe { auth_tag.snapshot::<TestTransport>() }, before);
                owner.send::<Independent>(&1).await.unwrap();
                assert_eq!(independent.recv::<Independent>().await.unwrap(), 1);
                owner.send::<Tick>(&()).await.unwrap();
                ticker.recv::<Tick>().await.unwrap();
                assert_eq!(capture.snapshot().len(), 1, "the loan is still unpolled");
                assert!(network.queue_is_empty());
                // SAFETY: ordinary messages completed; the loan has not polled.
                let reentered = unsafe { auth_tag.snapshot::<TestTransport>() };
                let result = poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx))).await;
                assert!(
                    matches!(result, Poll::Ready(Err(_))),
                    "the first poll must reject its own unavailable payload predecessor: {result:?}"
                );
                // SAFETY: the failed poll released its synchronous kernel lease.
                let failed = unsafe { auth_tag.snapshot::<TestTransport>() };
                assert_eq!(failed.completion_mutations, reentered.completion_mutations);
                assert_eq!(failed.steps, reentered.steps);
                assert_eq!(failed.choices, reentered.choices);
                // SAFETY: the same TestTransport actor remains alive and faulted.
                assert!(
                    unsafe { auth_tag.actor_call_cut::<TestTransport>() }
                        .fault
                        .is_some()
                );
            }
            let events = capture.finish();
            assert!(
                matches!(events.as_slice(), [
            OwnedFutureBoundary::Seal { .. },
            OwnedFutureBoundary::Poll { payload: polled, result: Poll::Ready(Err(_)) },
            OwnedFutureBoundary::Drop { payload: dropped },
        ] if *polled == payload_id && *dropped == payload_id),
                "{events:?}"
            );
            assert!(worker.recv::<Loan>().await.is_err());
            assert!(network.queue_is_empty());
            if !cfg!(miri) {
                let OwnedFutureBoundary::Seal { carrier, payload, label, region } = events[0] else {
                    unreachable!("checked constructor observation");
                };
                let tag = |value: crate::endpoint::proof_snapshot::owned_future::ValueIdentity| {
                    std::format!("({}, {}, {}, {})", value.actor_address, value.role, value.run, value.nonce)
                };
                let bounds = region.bounds();
                let source = std::format!(
                    "import Hibana.OwnedFutureInventory\n\n\
                     open Hibana\n\n\
                     def expectedUnpolledCarrier : Nat × Nat × Nat × Nat := {}\n\n\
                     def expectedUnpolledPayload : Nat × Nat × Nat × Nat := {}\n\n\
                     def actualUnpolledRequest : OwnedFutureInventory.Request\n  \
                     (Nat × Nat × Nat × Nat) (Bool × Nat × Nat × Nat) := {{\n  \
                     carrier := {}, payload := {}, label := {label},\n  \
                     borrow := ({}, {}, {}, {})\n}}\n\n\
                     def beforeUnpolledMove : EndpointTransfer.Observation\n  \
                     (Nat × Nat × Nat × Nat) Unit := EndpointTransfer.initial expectedUnpolledPayload\n\n\
                     def afterUnpolledMove : EndpointTransfer.Observation\n  \
                     (Nat × Nat × Nat × Nat) Unit := {{\n  \
                     usable := [], preparing := [actualUnpolledRequest.payload], acceptedFutures := [], inFlight := []\n}}\n\n\
                     theorem actualUnpolledConstructorFields :\n  \
                     actualUnpolledRequest.carrier = expectedUnpolledCarrier ∧ actualUnpolledRequest.payload = expectedUnpolledPayload ∧\n  \
                     actualUnpolledRequest.label = 1 ∧ actualUnpolledRequest.borrow.1 = true := by decide\n\n\
                     theorem actualUnpolledConstructorHistory :\n  \
                     OwnedFutureInventory.History [] beforeUnpolledMove [actualUnpolledRequest] afterUnpolledMove :=\n  \
                     .tail (.refl _ _) (.construct beforeUnpolledMove [] [] [] actualUnpolledRequest)\n\n\
                     theorem actualPrivatePayloadHasItsCapturedFuture :\n  \
                     ∃ request, request ∈ [actualUnpolledRequest] ∧ request.payload = expectedUnpolledPayload :=\n  \
                     OwnedFutureInventory.a_private_payload_has_its_actual_request\n    \
                     actualUnpolledConstructorHistory (by decide)\n\n\
                     theorem actualFutureInventoryCoversThisPrivatePayload :\n  \
                     OwnedFutureInventory.Covers [actualUnpolledRequest] afterUnpolledMove :=\n  \
                     OwnedFutureInventory.a_complete_future_history_covers_every_private_payload\n    \
                     actualUnpolledConstructorHistory\n",
                    tag(carrier_id), tag(payload_id), tag(carrier), tag(payload), region.is_whole(),
                    region.lane(256), bounds.start, bounds.end,
                );
                let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("OwnedFutureInventoryGenerated.lean"), source).unwrap();
            }
        });
}
