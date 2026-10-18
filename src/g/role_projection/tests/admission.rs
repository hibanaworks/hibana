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

            // Append-only qualification evidence. Endpoint operations never
            // inspect this log or use it to decide what may run next.
            #[cfg(not(miri))]
            let commits = std::cell::RefCell::new(std::vec::Vec::new());

            // SAFETY: every observation retains this pinned TestTransport actor.
            let initial = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(initial.1, [true, false, false, false]);
            owner.send::<Loan>(db).await.unwrap();
            #[cfg(not(miri))]
            let received_wire = network.queued_frame(1);
            let mut db = worker.recv::<Loan>().await.unwrap();
            // SAFETY: the same actor remains pinned and has no active kernel call.
            let received = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(received.1, [false, true, true, false]);
            #[cfg(not(miri))]
            commits
                .borrow_mut()
                .push((0, 1, initial.0.clone(), received.0.clone(), received_wire));

            let (db_done, worker_done) = tokio::join!(
                async {
                    // SAFETY: read before borrowing this live actor's DB view.
                    #[cfg(not(miri))]
                    let before = unsafe { actor_snapshot(&db) };
                    db.send::<Read>(&17).await.unwrap();
                    // SAFETY: this distinct view retains the same actor.
                    let observation = unsafe { (actor_snapshot(&db), actor_admissions(&db)) };
                    #[cfg(not(miri))]
                    commits.borrow_mut().push((
                        1,
                        10,
                        before,
                        observation.0.clone(),
                        network.queued_frame(3),
                    ));
                    observation
                },
                async {
                    // SAFETY: read before borrowing WORKER's live view.
                    #[cfg(not(miri))]
                    let before = unsafe { actor_snapshot(&worker) };
                    worker.send::<Value>(&19).await.unwrap();
                    // SAFETY: the shared actor lease was released by send.
                    let observation = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
                    #[cfg(not(miri))]
                    commits.borrow_mut().push((
                        2,
                        1,
                        before,
                        observation.0.clone(),
                        network.queued_frame(4),
                    ));
                    observation
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
            #[cfg(not(miri))]
            commits.borrow_mut().push((
                3,
                1,
                joined.0.clone(),
                returned.0.clone(),
                network.queued_frame(0),
            ));
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
                    "import Hibana.ActorCommitHistory\n\nset_option maxRecDepth 10000\n\n\
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
                source.push_str(r#"
def useData : Hibana.DecodedActorOrder :=
  (admissionImage.decodeActorOrder? admissionChoreo).get (by decide)
def usePath : Hibana.EndpointDelegation.NormalizedPath :=
  (useData.normalized.paths[0]?).get (by decide)
def readRow : Hibana.DecodedActorRow := (useData.rows[1]?).get (by decide)
def returnRow : Hibana.DecodedActorRow := (useData.rows[3]?).get (by decide)
def resourceKey : Hibana.EndpointDelegation.EndpointKey := { component := 0, role := 10 }
def readBinding : Hibana.EndpointDelegation.Binding :=
  ((usePath.bindings.find? fun entry => entry.1 == readRow.actionId.event).get (by decide)).2
def returnBinding : Hibana.EndpointDelegation.Binding :=
  ((usePath.bindings.find? fun entry => entry.1 == returnRow.actionId.event).get (by decide)).2

theorem readyReadExcludesReturnInEverySnapshot (snapshot : Hibana.ActorProgressObservation)
    (ready : Hibana.sourceRowReady useData usePath snapshot readRow = true) :
    Hibana.sourceRowReady useData usePath snapshot returnRow = false :=
  Hibana.checked_source_ready_use_excludes_another_handoff
    (image := admissionImage) (choreo := admissionChoreo)
    (loan := returnBinding) (useBinding := readBinding) (key := resourceKey)
    (by decide) ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) (by decide)
    (by decide) (Or.inl (by decide)) (by decide) ready

theorem independentWorkCanRemainReadyAlongsideTheRead :
    Hibana.sourceRowReady useData usePath received.snapshot readRow = true ∧
      Hibana.sourceRowReady useData usePath received.snapshot
        ((useData.rows[2]?).get (by decide)) = true := by decide

theorem arbitraryLiveReadSuffixKeepsTheExactValue
    {View Frame : Type} {value : View}
    {first final : Hibana.EndpointTransfer.Observation View Frame}
    (history : Hibana.EndpointTransfer.Execution first final)
    (present : Hibana.TransferValueLocated value first)
    (observed : ∀ prior next action,
      Hibana.EndpointTransfer.Execution first prior → Hibana.EndpointTransfer.Step action prior next →
      Hibana.EndpointTransfer.Execution next final →
      Hibana.SourceUseBoundaryEvidence admissionImage admissionChoreo useData usePath resourceKey readRow action) :
    Hibana.TransferValueLocated value final :=
  Hibana.checked_live_source_suffix_preserves_the_exact_received_value
    (image := admissionImage) (choreo := admissionChoreo)
    (data := useData) (path := usePath) (key := resourceKey) (useRow := readRow) (useBinding := readBinding)
    (by decide) ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide)
    (Or.inl (by decide)) (by decide) (by decide) history present observed

theorem readyReturnIsAnObservedSourceCut :
    joined.snapshot.viewIssued 10 = true ∧
      Hibana.sourceRowReady useData usePath joined.snapshot returnRow = true := by decide

theorem readyReturnOnlyAcceptsItsSourceAction
    {transfer : Hibana.DecodedActorRow} {loan : Hibana.EndpointDelegation.Binding}
    {snapshot : Hibana.ActorProgressObservation}
    (member : transfer ∈ useData.activeRows usePath)
    (located : usePath.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (payload : loan.message.payload = .endpoint resourceKey)
    (sending : transfer.actionId.side = .send)
    (ready : Hibana.sourceRowReady useData usePath snapshot transfer = true)
    (returnReady : Hibana.sourceRowReady useData usePath snapshot returnRow = true) :
    transfer.actionId = returnRow.actionId ∧ loan = returnBinding :=
  Hibana.checked_ready_owned_send_and_acceptance_name_the_same_action
    (image := admissionImage) (choreo := admissionChoreo)
    (intended := returnBinding) (key := resourceKey)
    (by decide) ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) member (by decide) located (by decide) payload (by decide) sending (by decide) ready returnReady

theorem arbitraryReadyReturnKeepsValueOrItsRequiredBoundary
    {View Frame : Type} {value : View}
    {first final : Hibana.EndpointTransfer.Observation View Frame}
    (history : Hibana.EndpointTransfer.Execution first final)
    (present : Hibana.TransferValueLocated value first)
    (observed : ∀ prior next action,
      Hibana.EndpointTransfer.Execution first prior → Hibana.TransferValueLocated value prior →
      Hibana.EndpointTransfer.Step action prior next → Hibana.EndpointTransfer.Execution next final →
      Hibana.SourceHandoffBoundaryEvidence admissionImage admissionChoreo useData usePath resourceKey returnRow action) :
    Hibana.TransferValueLocated value final ∨
      ∃ prior next action, Hibana.EndpointTransfer.Execution first prior ∧
        Hibana.EndpointTransfer.Step action prior next ∧ Hibana.TransferValueLocated value prior ∧
        Hibana.EndpointTransfer.Execution next final ∧
        Hibana.SourceHandoffBoundaryOutcome admissionImage admissionChoreo useData usePath resourceKey returnRow action ∧
        (action = .accept → ∃ frame, Hibana.TransferFrameLocated frame next) :=
  Hibana.ready_source_handoff_keeps_its_value_or_takes_its_required_boundary
    (image := admissionImage) (choreo := admissionChoreo)
    (data := useData) (path := usePath) (key := resourceKey) (target := returnRow) (intended := returnBinding)
    (by decide) ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide) history present observed

theorem everyReadyRowHasItsSourceDerivedInitialIssueOrReceipt
    {row : Hibana.DecodedActorRow} {binding : Hibana.EndpointDelegation.Binding}
    {key : Hibana.EndpointDelegation.EndpointKey} {snapshot : Hibana.ActorProgressObservation}
    (member : row ∈ useData.activeRows usePath)
    (located : usePath.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (required : match row.actionId.side with
      | .receive => binding.message.receiver = key
      | .send => binding.message.sender = key ∨ binding.message.payload = .endpoint key
      | .local => binding.message.sender = key)
    (ready : Hibana.sourceRowReady useData usePath snapshot row = true) :
    ∃ (issuance : List Hibana.EndpointDelegation.Issuance)
      (run : Hibana.EndpointDelegation.Elaboration) (index : Nat)
      (symbolic : Hibana.EndpointDelegation.SymbolicBinding),
      Hibana.EndpointDelegation.elaborate useData.source.resources
        (Hibana.EndpointDelegation.initialOwners useData.source.resources)
        usePath.selections useData.source 0 = some run ∧
      run.bindings[index]? = some (row.actionId.event, symbolic) ∧
      Hibana.EndpointDelegation.instantiateBinding issuance symbolic = some binding ∧
      run.equations.all (Hibana.EndpointDelegation.equationAccepted issuance) = true ∧
      (run.bindings.mapM fun (event, symbolic) => do
        let binding ← Hibana.EndpointDelegation.instantiateBinding issuance symbolic
        some (event, binding)) = some usePath.bindings ∧
      (Hibana.EndpointDelegation.instantiate issuance
        ((Hibana.EndpointDelegation.initialOwners useData.source.resources) key) = some useData.actor ∨
        ∃ previousRow previousBinding,
          previousRow ∈ useData.activeRows usePath ∧ previousRow.actionId.side = .receive ∧
          usePath.bindings.find? (fun entry => entry.1 == previousRow.actionId.event) =
            some (previousRow.actionId.event, previousBinding) ∧
          previousBinding.message.payload = .endpoint key ∧ previousBinding.receiverActor = useData.actor ∧
          previousRow.eventId ∈ useData.sourcePredecessors usePath row.eventId ∧
          snapshot.doneState.done previousRow.eventId = true) :=
  Hibana.source_ready_resource_row_has_initial_authority_or_a_completed_receipt
    (image := admissionImage) (choreo := admissionChoreo)
    (by decide) ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) member located required ready

"#);
                let commits = commits.borrow();
                assert_eq!(commits.len(), 4);
                assert_eq!(commits[0].0, 0);
                assert_eq!(commits[3].0, 3);
                assert_ne!(commits[1].0, commits[2].0);
                for (index, (event, view, before, after, (header, payload))) in
                    commits.iter().enumerate()
                {
                    assert!(!before.steps[*event].done);
                    assert!(after.steps[*event].done);
                    assert_eq!(after.steps[*event].view, *view);
                    source.push_str(&std::format!(
                        "def observedCommit{index} : Hibana.ActorCommitRecord := {{\n  \
                         step := {{\n    pathIndex := 0\n    event := {event}\n    view := {view}\n    \
                         before := {}\n    after := {}\n  }}\n  \
                         wire := {{\n    session := {}\n    lane := {}\n    sender := {}\n    receiver := {}\n    \
                         frameLabel := {}\n    payload := {payload:?}\n  }} }}\n\n",
                        super::commit::observation_source(before),
                        super::commit::observation_source(after),
                        header.session().raw(),
                        header.lane(),
                        header.source_role(),
                        header.target_role(),
                        header.label().raw(),
                    ));
                }
                source.push_str(r#"
def observedCommits : List Hibana.ActorCommitRecord :=
  [observedCommit0, observedCommit1, observedCommit2, observedCommit3]

theorem observedCommitHistoryChecked :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot returned.snapshot observedCommits = true := by decide

theorem observedReceiptHistoryChecked :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot received.snapshot [observedCommit0] = true := by decide

theorem omittedReceiptCommitRejected :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot returned.snapshot observedCommits.tail = false := by decide

theorem unrecordedGapCompletionRejected :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot received.snapshot [] = false := by decide

theorem corruptedReceiptWireRejected :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot returned.snapshot
      ({ observedCommit0 with wire := { observedCommit0.wire with payload := [255, 0, 0] } } ::
        observedCommits.tail) = false := by decide

theorem changedObservationOrderRejected :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot returned.snapshot
      [observedCommit0, observedCommit2, observedCommit1, observedCommit3] = false := by decide

def receiptRow : Hibana.DecodedActorRow := (useData.rows[0]?).get (by decide)
def receiptBinding : Hibana.EndpointDelegation.Binding :=
  ((usePath.bindings.find? fun entry => entry.1 == receiptRow.actionId.event).get (by decide)).2

theorem everyReadyReadHistoryContainsTheWireCheckedReceipt
    {first final : Hibana.ActorProgressObservation} {records : List Hibana.ActorCommitRecord}
    (history : Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053 first final records = true)
    (unfinished : first.doneState.done receiptRow.eventId = false)
    (ready : Hibana.sourceRowReady useData usePath final readRow = true) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = receiptRow.eventId ∧
      record.step.ownedCheck admissionImage admissionChoreo record.wire 9053 = true ∧
      record.step.SourceOrdered admissionImage admissionChoreo ∧
      Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053 record.step.after final later = true ∧
      (∀ next ∈ later, next.step.event ≠ receiptRow.eventId) ∧
      record.step.after.doneState.done receiptRow.eventId = true ∧
      ∀ next ∈ later, next.step.before.doneState.done receiptRow.eventId = true ∧
        next.step.after.doneState.done receiptRow.eventId = true := by
  have completed := List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 receiptRow.eventId (by decide)
  exact Hibana.completed_owned_source_row_has_a_wire_checked_commit
    (path := usePath) (binding := receiptBinding) (key := resourceKey)
    ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) history unfinished completed

theorem observedReadyReadHasTheWireCheckedReceipt :
    ∃ earlier record later, [observedCommit0] = earlier ++ record :: later ∧
      record.step.event = receiptRow.eventId ∧
      record.step.ownedCheck admissionImage admissionChoreo record.wire 9053 = true ∧
      record.step.SourceOrdered admissionImage admissionChoreo ∧
      Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
        record.step.after received.snapshot later = true ∧
      (∀ next ∈ later, next.step.event ≠ receiptRow.eventId) ∧
      record.step.after.doneState.done receiptRow.eventId = true ∧
      ∀ next ∈ later, next.step.before.doneState.done receiptRow.eventId = true ∧
        next.step.after.doneState.done receiptRow.eventId = true :=
  everyReadyReadHistoryContainsTheWireCheckedReceipt observedReceiptHistoryChecked (by decide) (by decide)

"#);
                let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("ActorAdmissionGenerated.lean"), source).unwrap();
            }
            drop(db);
        });
}
