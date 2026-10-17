//! Compare actual shared-cursor admission with independently decoded source order.
use super::*;
use crate::{
    endpoint::proof_snapshot::{actor_admissions, actor_snapshot},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn source_ready_rows_match_actual_admission_across_a_parallel_owned_visit() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<g::Par<g::Send<10, 3, Read>, g::Send<1, 4, Value>>, g::Send<1, 0, Loan>>,
    >;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 8>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 8>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut peer_slab = [0; 32 * 1024];
            let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
            let mut peer_storage = SessionKitStorage::<TestTransport>::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let peer_rv = peer_storage
                .init()
                .rendezvous(&mut peer_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9053);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = peer_rv.enter(sid, &worker_program).unwrap();
            let mut reader = peer_rv.enter(sid, &reader_program).unwrap();
            let mut peer = peer_rv.enter(sid, &peer_program).unwrap();

            // SAFETY: every observation retains this pinned TestTransport actor.
            let initial = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(initial.1, [true, false, false, false]);
            owner.send::<Loan>(db).await.unwrap();
            let mut db = worker.recv::<Loan>().await.unwrap();
            // SAFETY: the same actor remains pinned and has no active kernel call.
            let received = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(received.1, [false, true, true, false]);

            let (db_done, worker_done) = tokio::join!(
                async {
                    db.send::<Read>(&17).await.unwrap();
                    // SAFETY: this distinct view retains the same actor.
                    unsafe { (actor_snapshot(&db), actor_admissions(&db)) }
                },
                async {
                    worker.send::<Value>(&19).await.unwrap();
                    // SAFETY: the shared actor lease was released by send.
                    unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) }
                },
            );
            assert_eq!(reader.recv::<Read>().await.unwrap(), 17);
            assert_eq!(peer.recv::<Value>().await.unwrap(), 19);
            // One par side can finish without consuming the other's step.
            assert!(
                db_done.1 == [false, false, true, false]
                    || worker_done.1 == [false, true, false, false]
            );
            // SAFETY: both futures have finished; the worker view is still live.
            let joined = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(joined.1, [false, false, false, true]);
            worker.send::<Loan>(db).await.unwrap();
            // SAFETY: returning DB leaves WORKER's own view alive.
            let returned = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(returned.1, [false, false, false, false]);
            let db = owner.recv::<Loan>().await.unwrap();
            assert!(network.queue_is_empty());

            #[cfg(not(miri))]
            {
                use crate::test_support::{
                    lean_choreo::LeanChoreo, lean_descriptor::descriptor_source,
                };
                let choreo = Steps::lean_source();
                let image = descriptor_source(RoleDescriptorRef::from_resident(
                    &RoleProjection::<1, Steps, 8>::IMAGE_REF,
                ));
                let mut source = std::format!(
                    "import Hibana.ActorAdmission\n\nset_option maxRecDepth 10000\n\n\
                     def admissionChoreo : Hibana.Choreo := {choreo}\n\n\
                     def admissionImage : Hibana.RustDescriptorImage := {image}\n\n"
                );
                for (name, (snapshot, admitted)) in [
                    ("initial", initial),
                    ("received", received),
                    ("dbDone", db_done),
                    ("workerDone", worker_done),
                    ("joined", joined),
                    ("returned", returned),
                ] {
                    source.push_str(&std::format!(
                        "def {name} : Hibana.ActorAdmissionObservation := {{ snapshot := {}, \
                         pathIndex := 0, admitted := {admitted:?} }}\n\n\
                         theorem {name}Checked : {name}.check admissionImage admissionChoreo = true := by decide\n\n\
                         theorem {name}SourceReadiness :\n  \
                         ∃ data path, admissionImage.decodeActorOrder? admissionChoreo = some data ∧\n    \
                         data.normalized.paths[{name}.pathIndex]? = some path ∧\n    \
                         ∀ row ∈ data.activeRows path,\n      \
                         {name}.snapshot.doneState.PrefixClosed (data.sourcePredecessors path)\n        \
                         (data.sourcePredecessors path row.eventId) ∧\n      \
                         {name}.admitted[row.eventId]? = some (Hibana.sourceRowReady data path {name}.snapshot row) ∧\n      \
                         (Hibana.sourceRowReady data path {name}.snapshot row = true →\n        \
                         Hibana.sourceRowViewsIssued path {name}.snapshot row = true) :=\n  \
                         Hibana.accepted_actor_admission_has_exact_source_readiness {name}Checked\n\n",
                        super::commit::observation_source(&snapshot),
                    ));
                }
                source.push_str(
                    "theorem missingAdmissionRowRejected :\n  \
                     ({ received with admitted := received.admitted.take 3 } : Hibana.ActorAdmissionObservation).check\n    \
                     admissionImage admissionChoreo = false := by decide\n\n\
                     theorem prematureReturnRejected :\n  \
                     ({ received with admitted := received.admitted.set 3 true } : Hibana.ActorAdmissionObservation).check\n    \
                     admissionImage admissionChoreo = false := by decide\n\n\
                     theorem lostParallelReadinessRejected :\n  \
                     ({ received with admitted := received.admitted.set 1 false } : Hibana.ActorAdmissionObservation).check\n    \
                     admissionImage admissionChoreo = false := by decide\n\n\
                     theorem completedStepReplayRejected :\n  \
                     ({ received with admitted := received.admitted.set 0 true } : Hibana.ActorAdmissionObservation).check\n    \
                     admissionImage admissionChoreo = false := by decide\n\n\
                     def missingPayloadView : Hibana.ActorAdmissionObservation :=\n  \
                     { received with snapshot := { received.snapshot with views := received.snapshot.views.map (fun view =>\n    \
                     if view.role = 10 then { view with run := 0, nonce := view.nonce + 1 } else view) } }\n\n\
                     theorem missingPayloadViewSnapshotValid :\n  \
                     missingPayloadView.snapshot.valid = true := by decide\n\n\
                     theorem missingPayloadViewRejected :\n  \
                     missingPayloadView.check admissionImage admissionChoreo = false := by decide\n\n\
                     theorem receiverViewIssuedOnlyAfterReceipt :\n  \
                     initial.snapshot.viewIssued 10 = false ∧ received.snapshot.viewIssued 10 = true ∧\n    \
                     returned.snapshot.viewIssued 10 = false := by decide\n\n",
                );
                let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("ActorAdmissionGenerated.lean"), source).unwrap();
            }
            drop(db);
        });
}
