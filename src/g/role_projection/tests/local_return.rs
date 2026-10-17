use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
    },
    test_support::transport::TestTransport,
};
use core::{future::Future, future::poll_fn, pin::pin, task::Poll};

#[test]
fn a_local_branch_waits_for_another_view_to_receive_its_entry_resource() {
    type Tick = g::Msg<42, ()>;
    type Tock = g::Msg<43, ()>;
    type AlternateRead = g::Msg<44, u32>;
    type Independent = g::Msg<45, u32>;
    type ExtraLoan = g::Msg<46, Endpoint<'static, 12>>;
    type Work = g::Seq<
        g::Roll<
            g::Resolve<
                g::Route<
                    g::Seq<g::Send<0, 0, Tick>, g::Send<10, 3, Read>>,
                    g::Seq<g::Send<0, 0, Tock>, g::Send<10, 3, AlternateRead>>,
                >,
                711,
            >,
        >,
        g::Seq<g::Send<11, 4, Loan>, g::Seq<g::Send<4, 11, Loan>, g::Send<0, 2, OtherLoan>>>,
    >;
    type Steps =
        g::Seq<g::Par<Work, g::Roll<g::Send<12, 8, Independent>>>, g::Send<0, 2, ExtraLoan>>;
    const ROWS: usize = <Steps as g::ProgramShape>::SOURCE_ROW_COUNT;
    let () = g::ProgramProjection::<Steps, ROWS>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, ROWS>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, ROWS>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, ROWS>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, ROWS>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, ROWS>::IMAGE_REF,
    );
    let independent_program = crate::global::role_program::role_program_from_image::<12>(
        &RoleProjection::<12, Steps, ROWS>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<8>(
        &RoleProjection::<8, Steps, ROWS>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let decision = core::cell::Cell::new(7u32);
            let mut slab = [0u8; 128 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            rv.set_resolver(
                &owner_program,
                ResolverRef::<711>::decision_state(&decision, |value| {
                    Ok(if value.get() % 2 == 1 {
                        DecisionArm::Left
                    } else {
                        DecisionArm::Right
                    })
                }),
            )
            .unwrap();
            let sid = SessionId::new(9038);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            let mut independent = rv.enter(sid, &independent_program).unwrap();
            let mut peer = rv.enter(sid, &peer_program).unwrap();
            owner.offer().await.unwrap().recv::<Tick>().await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            decision.set(8);
            // SAFETY: every observed view belongs to this TestTransport actor.
            let before_wait = unsafe { actor_snapshot(&auth) };
            let after_pending = {
                let preview = owner.offer().await.unwrap();
                let mut paused = pin!(preview.recv::<Tock>());
                poll_fn(|cx| {
                    assert!(paused.as_mut().poll(cx).is_pending());
                    // SAFETY: AUTH is a live view of the same retained actor.
                    Poll::Ready(unsafe { actor_snapshot(&auth) })
                })
                .await
            };
            // SAFETY: the runtime and AUTH remain live after the preview drops.
            let after_drop = unsafe { actor_snapshot(&auth) };
            for observed in [&after_pending, &after_drop] {
                assert_eq!(observed.run, before_wait.run);
                assert_eq!(observed.counter, before_wait.counter);
                assert_eq!(observed.live_views, before_wait.live_views);
                assert_eq!(observed.views, before_wait.views);
                assert_eq!(observed.choices, before_wait.choices);
                assert_eq!(observed.steps.len(), before_wait.steps.len());
                for (actual, previous) in observed.steps.iter().zip(&before_wait.steps) {
                    assert_eq!(
                        (actual.view, actual.action, actual.done),
                        (previous.view, previous.action, previous.done)
                    );
                }
            }
            let branch = owner.offer().await.unwrap();
            assert_eq!(branch.label(), 43);
            let mut restart = pin!(branch.recv::<Tock>());
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    outcome.is_pending(),
                    "the local branch must wait for AUTH's receipt: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
            // An external input change must not replace this pending preview.
            decision.set(9);
            for value in [2, 3] {
                independent.send::<Independent>(&value).await.unwrap();
                assert_eq!(peer.recv::<Independent>().await.unwrap(), value);
            }
            worker.send::<Loan>(db).await.unwrap();
            let (after_local, (db, before_local), _) =
                tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        restart.await.unwrap();
                        // SAFETY: the independent view retains this same actor.
                        unsafe { actor_snapshot(&independent) }
                    },
                    async {
                        let mut db = auth.recv::<Loan>().await.unwrap();
                        // SAFETY: receipt returns before this read-only snapshot.
                        let before = unsafe { actor_snapshot(&auth) };
                        db.send::<AlternateRead>(&8).await.unwrap();
                        (db, before)
                    },
                    async { assert_eq!(reader.recv::<AlternateRead>().await.unwrap(), 8) },
                )
            })
            .await
            .expect("a local branch must not prevent another view's return");
            assert!(network.queue_is_empty());
            let event = before_local.steps.iter().position(|step| {
                step.view == 0 && matches!(step.action, LocalAction::Local { label: 43, .. })
            }).unwrap();
            assert!(!before_local.steps[event].done);
            assert!(before_local.steps[event].reentry_scope.is_some());
            #[cfg(not(miri))]
            {
                use crate::test_support::{
                    lean_choreo::LeanChoreo, lean_descriptor::descriptor_source,
                };
                let scope = before_local.steps[event].reentry_scope.unwrap();
                let mut source = std::format!(
                    "import Hibana.ActorReentryCommit\n\nset_option maxRecDepth 10000\n\n\
                     def localReturnChoreo : Hibana.Choreo := {}\n\n\
                     def localReturnImage : Hibana.RustDescriptorImage := {}\n\n",
                    Steps::lean_source(),
                    descriptor_source(RoleDescriptorRef::from_resident(&RoleProjection::<0, Steps, ROWS>::IMAGE_REF)),
                );
                for (name, snapshot) in [
                    ("localBeforeWait", &before_wait),
                    ("localAfterPending", &after_pending),
                    ("localAfterDrop", &after_drop),
                    ("localBeforeResume", &before_local),
                    ("localAfterResume", &after_local),
                ] {
                    source.push_str(&std::format!("def {name} : Hibana.ActorProgressObservation := {}\n\n", super::commit::observation_source(snapshot)));
                }
                source.push_str(&std::format!(
                    "def localReturnCommit : Hibana.ActorReentryCommitObservation := {{\n  \
                     previousPath := 0, nextPath := 1, scope := {scope}, event := {event}, view := 0,\n  \
                     before := localBeforeResume, after := localAfterResume\n}}\n\n\
                     theorem localReceiptIndexAccepted : localReturnImage.ownedRecoveryIndexCheck = true := by decide\n\n\
                     theorem localPendingPreservesProgress : localAfterPending = localBeforeWait := by decide\n\n\
                     theorem localDropPreservesProgress : localAfterDrop = localBeforeWait := by decide\n\n\
                     theorem localResumeBeforeValid : localBeforeResume.valid = true := by decide\n\n\
                     theorem localReturnCommitAccepted : localReturnCommit.check localReturnImage localReturnChoreo = true := by decide\n\n\
                     theorem localReturnCommitSourceOrdered : localReturnCommit.SourceOrdered localReturnImage localReturnChoreo :=\n  \
                     Hibana.accepted_body_reentry_restores_source_progress localReturnCommitAccepted\n\n\
                     def localMissingReset : Hibana.ActorReentryCommitObservation :=\n  \
                     {{ localReturnCommit with after := {{ localAfterResume with steps := localBeforeResume.steps }} }}\n\n\
                     theorem localMissingResetRejected : localMissingReset.check localReturnImage localReturnChoreo = false := by decide\n\n\
                     def localWrongViewCount : Hibana.ActorReentryCommitObservation :=\n  \
                     {{ localReturnCommit with after := {{ localAfterResume with liveViews := localAfterResume.liveViews + 1 }} }}\n\n\
                     theorem localWrongViewCountRejected : localWrongViewCount.check localReturnImage localReturnChoreo = false := by decide\n"
                ));
                source.push_str(
                    "\n\
                     theorem localEntryHeadersAccepted :\n  \
                     Hibana.observedRollEntryHeadersMatch localBeforeResume localReturnImage localReturnCommit.scope = true := by decide\n\n\
                     def localWithoutEntryHeader (snapshot : Hibana.ActorProgressObservation) : Hibana.ActorProgressObservation :=\n  \
                     { snapshot with\n    \
                     liveViews := snapshot.liveViews - 1\n    \
                     views := snapshot.views.map fun header =>\n      \
                     if header.role = 10 then { header with run := 0, nonce := Hibana.retireViewGeneration header.nonce } else header }\n\n\
                     def localMissingEntryHeader : Hibana.ActorReentryCommitObservation :=\n  \
                     { localReturnCommit with\n    \
                     before := localWithoutEntryHeader localBeforeResume\n    \
                     after := localWithoutEntryHeader localAfterResume }\n\n\
                     theorem localMissingEntryBeforeValid : localMissingEntryHeader.before.valid = true := by decide\n\n\
                     theorem localMissingEntryAfterValid : localMissingEntryHeader.after.valid = true := by decide\n\n\
                     theorem localMissingEntryHeaderRejected : localMissingEntryHeader.check localReturnImage localReturnChoreo = false := by decide\n",
                );
                let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&output).unwrap();
                std::fs::write(output.join("LocalReturnGenerated.lean"), source).unwrap();
            }
            assert_eq!(before_local.views, after_local.views);
            drop(db);
        });
}

#[test]
fn a_local_branch_rejects_a_receipt_that_needs_its_own_borrowed_view() {
    type Tick = g::Msg<42, ()>;
    type Tock = g::Msg<43, ()>;
    type AlternateRead = g::Msg<44, u32>;
    type Steps = g::Seq<
        g::Roll<
            g::Resolve<
                g::Route<
                    g::Seq<g::Send<0, 0, Tick>, g::Send<10, 3, Read>>,
                    g::Seq<g::Send<0, 0, Tock>, g::Send<10, 3, AlternateRead>>,
                >,
                712,
            >,
        >,
        g::Seq<g::Send<0, 4, Loan>, g::Send<4, 0, Loan>>,
    >;
    const ROWS: usize = <Steps as g::ProgramShape>::SOURCE_ROW_COUNT;
    let () = g::ProgramProjection::<Steps, ROWS>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, ROWS>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, ROWS>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, ROWS>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, ROWS>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let decision = ();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            rv.set_resolver(
                &owner_program,
                ResolverRef::<712>::decision_state(&decision, |_| Ok(DecisionArm::Left)),
            )
            .unwrap();
            let sid = SessionId::new(9039);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            owner.offer().await.unwrap().recv::<Tick>().await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            owner.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            let branch = owner.offer().await.unwrap();
            let result =
                tokio::time::timeout(core::time::Duration::from_secs(10), branch.recv::<Tick>())
                    .await;
            assert!(
                matches!(result, Ok(Err(_))),
                "the outstanding branch borrows the required receiving view: {result:?}"
            );
            assert!(network.queue_is_empty());
            drop(db);
        });
}
