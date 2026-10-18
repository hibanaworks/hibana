//! Compare actual shared-cursor admission with independently decoded source order.
use super::*;
use crate::{
    endpoint::proof_snapshot::{actor_admissions, actor_snapshot, view_tag},
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
            let initial_db_tag = view_tag(&db);
            // SAFETY: OWNER retains this pinned TestTransport actor.
            #[cfg(not(miri))]
            let owner_initial = unsafe { actor_snapshot(&owner) };

            // Append-only qualification evidence. Endpoint operations never
            // inspect this log or use it to decide what may run next.
            #[cfg(not(miri))]
            let commits = std::cell::RefCell::new(std::vec::Vec::new());

            // SAFETY: every observation retains this pinned TestTransport actor.
            let initial = unsafe { (actor_snapshot(&worker), actor_admissions(&worker)) };
            assert_eq!(initial.1, [true, false, false, false]);
            owner.send::<Loan>(db).await.unwrap();
            // SAFETY: OWNER keeps the original resident DB header pinned.
            assert!(!unsafe { initial_db_tag.matches_header() });
            // SAFETY: the completed send has released its kernel borrow.
            #[cfg(not(miri))]
            let owner_sent = unsafe { actor_snapshot(&owner) };
            #[cfg(not(miri))]
            let received_wire = network.queued_frame(1);
            let mut db = worker.recv::<Loan>().await.unwrap();
            let received_db_tag = view_tag(&db);
            // SAFETY: DB's actual received value retains this issued header.
            assert!(unsafe { received_db_tag.matches_header() });
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
            // SAFETY: WORKER retains the same actor after moving DB.
            assert!(!unsafe { received_db_tag.matches_header() });
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
            // SAFETY: OWNER's live view pins the receiving actor.
            #[cfg(not(miri))]
            let owner_before_return = unsafe { actor_snapshot(&owner) };
            let db = owner.recv::<Loan>().await.unwrap();
            let returned_db_tag = view_tag(&db);
            // SAFETY: both actor allocations remain pinned. Reissuance
            // must not resurrect either consumed Rust value's old handle.
            unsafe {
                assert!(returned_db_tag.matches_header());
                assert!(!initial_db_tag.matches_header());
                assert!(!received_db_tag.matches_header());
            }
            // SAFETY: the actual returned DB value pins its new owner actor.
            #[cfg(not(miri))]
            let owner_returned = unsafe { actor_snapshot(&db) };
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
                    "import Hibana.ActorCommitHistory\nimport Hibana.ResourceValueCorrespondence\nimport Hibana.SourceReadyReceipt\n\nset_option maxRecDepth 10000\n\n\
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

def actualReceiptFacts : Hibana.OwnedCommitFacts :=
  (observedCommit0.step.ownedFacts? admissionImage admissionChoreo).get (by decide)

theorem actualReceiptHasTheSourceResourceAndSide :
    actualReceiptFacts.resource = resourceKey ∧ actualReceiptFacts.side = .receive :=
  Hibana.checked_owned_source_row_has_its_exact_resource_and_side
    (image := admissionImage) (choreo := admissionChoreo) (data := useData)
    (path := usePath) (row := receiptRow) (binding := receiptBinding)
    ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) (by decide)
    ((Option.some_get (x := observedCommit0.step.ownedFacts? admissionImage admissionChoreo) (by decide)).symm)

-- These applications derive the receipt from source readiness and the real
-- commit history. They do not take an admission observation or issued view.
theorem actualSourceReadyReadDerivesItsReceipt :
    Hibana.SourceReadyReceiptEvidence admissionImage admissionChoreo 9053 useData usePath
      readRow readBinding resourceKey [observedCommit0] received.snapshot :=
  Hibana.source_ready_resource_has_initial_issuer_or_current_wire_receipt
    (image := admissionImage) (choreo := admissionChoreo) (data := useData)
    (path := usePath) (row := readRow) (binding := readBinding) (key := resourceKey)
    (first := initial.snapshot) (final := received.snapshot) (records := [observedCommit0])
    (by decide)
    ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide)
    (by
      change readBinding.message.sender = resourceKey ∨ readBinding.message.payload = .endpoint resourceKey
      exact .inl (by decide))
    (by decide) observedReceiptHistoryChecked
    (by
      intro receipt member _ _ _ _
      have pending := List.all_eq_true.mp
        (show (useData.activeRows usePath).all
          (fun row => !initial.snapshot.doneState.done row.eventId) = true from by decide)
        receipt member
      simpa using pending)

theorem actualReadyReturnCommitHistoryChecked :
    Hibana.actorCommitHistoryCheck admissionImage admissionChoreo 9053
      initial.snapshot joined.snapshot [observedCommit0, observedCommit1, observedCommit2] = true := by decide

theorem actualSourceReadyReturnDerivesItsReceipt :
    Hibana.SourceReadyReceiptEvidence admissionImage admissionChoreo 9053 useData usePath
      returnRow returnBinding resourceKey [observedCommit0, observedCommit1, observedCommit2]
      joined.snapshot :=
  Hibana.source_ready_resource_has_initial_issuer_or_current_wire_receipt
    (image := admissionImage) (choreo := admissionChoreo) (data := useData)
    (path := usePath) (row := returnRow) (binding := returnBinding) (key := resourceKey)
    (first := initial.snapshot) (final := joined.snapshot)
    (records := [observedCommit0, observedCommit1, observedCommit2])
    (by decide)
    ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide)
    (by
      change returnBinding.message.sender = resourceKey ∨ returnBinding.message.payload = .endpoint resourceKey
      exact .inr (by decide))
    (by decide) actualReadyReturnCommitHistoryChecked
    (by
      intro receipt member _ _ _ _
      have pending := List.all_eq_true.mp
        (show (useData.activeRows usePath).all
          (fun row => !initial.snapshot.doneState.done row.eventId) = true from by decide)
        receipt member
      simpa using pending)

"#);
                let owner_image = descriptor_source(RoleDescriptorRef::from_resident(
                    &RoleProjection::<0, Steps, 8>::IMAGE_REF,
                ));
                source.push_str(&std::format!(
                    "def ownerImage : Hibana.RustDescriptorImage := {owner_image}\n\n"
                ));
                for (name, snapshot) in [
                    ("ownerInitial", &owner_initial),
                    ("ownerSent", &owner_sent),
                    ("ownerBeforeReturn", &owner_before_return),
                    ("ownerReturned", &owner_returned),
                ] {
                    source.push_str(&std::format!(
                        "def {name} : Hibana.ActorProgressObservation := {}\n\n",
                        super::commit::observation_source(snapshot),
                    ));
                }
                let owner_send_event = owner_initial
                    .steps
                    .iter()
                    .position(|step| {
                        matches!(step.action, LocalAction::Send { eff_index, .. } if eff_index.dense_ordinal() == 0)
                    })
                    .unwrap();
                let owner_return_event = owner_before_return
                    .steps
                    .iter()
                    .position(|step| {
                        matches!(step.action, LocalAction::Recv { eff_index, .. } if eff_index.dense_ordinal() == 3)
                    })
                    .unwrap();
                for (name, event, before, after, wire) in [
                    (
                        "ownerAccept",
                        owner_send_event,
                        "ownerInitial",
                        "ownerSent",
                        "observedCommit0.wire",
                    ),
                    (
                        "ownerReturn",
                        owner_return_event,
                        "ownerBeforeReturn",
                        "ownerReturned",
                        "observedCommit3.wire",
                    ),
                ] {
                    source.push_str(&std::format!(
                        "def {name} : Hibana.ActorCommitObservation := {{\n  \
                         pathIndex := 0\n  event := {event}\n  view := 0\n  \
                         before := {before}\n  after := {after}\n}}\n\n\
                         theorem {name}Checked : {name}.ownedCheck ownerImage admissionChoreo {wire} 9053 = true := by decide\n\n"
                    ));
                }
                source.push_str(r#"
def ownerAcceptFacts : Hibana.OwnedCommitFacts :=
  (ownerAccept.ownedFacts? ownerImage admissionChoreo).get (by decide)
def workerReceiveFacts : Hibana.OwnedCommitFacts :=
  (observedCommit0.step.ownedFacts? admissionImage admissionChoreo).get (by decide)
def workerAcceptFacts : Hibana.OwnedCommitFacts :=
  (observedCommit3.step.ownedFacts? admissionImage admissionChoreo).get (by decide)
def ownerReturnFacts : Hibana.OwnedCommitFacts :=
  (ownerReturn.ownedFacts? ownerImage admissionChoreo).get (by decide)
def initialDbView : Hibana.ActorViewObservation :=
  (Hibana.observedActorView? ownerInitial 10).get (by decide)
def initialDbTag : Nat × Nat × Nat × Nat := (ownerInitial.actor, initialDbView.header)
def receivedDbTag : Nat × Nat × Nat × Nat := (received.snapshot.actor, workerReceiveFacts.afterView.header)
def returnedDbTag : Nat × Nat × Nat × Nat := (ownerReturned.actor, ownerReturnFacts.afterView.header)

def dbHeaders0 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? ownerInitial 10 = some value ∨
    Hibana.observedResourceTag? initial.snapshot 10 = some value
def dbHeaders1 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? ownerSent 10 = some value ∨
    Hibana.observedResourceTag? initial.snapshot 10 = some value
def dbHeaders2 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? received.snapshot 10 = some value ∨
    Hibana.observedResourceTag? ownerSent 10 = some value
def dbHeaders3 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? joined.snapshot 10 = some value ∨
    Hibana.observedResourceTag? ownerBeforeReturn 10 = some value
def dbHeaders4 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? returned.snapshot 10 = some value ∨
    Hibana.observedResourceTag? ownerBeforeReturn 10 = some value
def dbHeaders5 (value : Nat × Nat × Nat × Nat) : Prop :=
  Hibana.observedResourceTag? ownerReturned 10 = some value ∨
    Hibana.observedResourceTag? returned.snapshot 10 = some value

def dbStart : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation :=
  Hibana.EndpointTransfer.initial initialDbTag
def dbOwnerSealed : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbStart with usable := [], preparing := [initialDbTag] }
def dbOwnerAccepted : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbOwnerSealed with
  preparing := []
  acceptedFutures := [initialDbTag]
  inFlight := [observedCommit0.wire] }
def dbOwnerReleased : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbOwnerAccepted with acceptedFutures := [] }
def dbAtWorker : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbOwnerReleased with inFlight := [], usable := [receivedDbTag] }
def dbWorkerSealed : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbAtWorker with usable := [], preparing := [receivedDbTag] }
def dbWorkerAccepted : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbWorkerSealed with
  preparing := []
  acceptedFutures := [receivedDbTag]
  inFlight := [observedCommit3.wire] }
def dbWorkerReleased : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbWorkerAccepted with acceptedFutures := [] }
def dbAtOwner : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation := { dbWorkerReleased with inFlight := [], usable := [returnedDbTag] }

theorem observedInitialHeadersIdentifyTheSoleInitialTag :
    ∀ value, dbHeaders0 value ↔ value = initialDbTag := by
  have ownerAt : Hibana.observedResourceTag? ownerInitial 10 = some initialDbTag := by decide
  have workerAt : Hibana.observedResourceTag? initial.snapshot 10 = none := by decide
  intro value
  simp only [dbHeaders0, ownerAt, workerAt, Option.some.injEq, reduceCtorEq, or_false, eq_comm]

theorem observedFirstLoanHasItsCheckedHeaderTransferHistory :
    Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders2 dbAtWorker := by
  have sealed : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders0 dbOwnerSealed :=
    .tail (.refl _ _) (.seal dbStart [] [] initialDbTag) (fun _ present => present)
  have accepted : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders0 dbOwnerAccepted :=
    .tail sealed (.accept dbOwnerSealed [] [] initialDbTag observedCommit0.wire) (fun _ present => present)
  have publication := Hibana.a_checked_owned_send_supplies_its_local_publication_header_evidence
    (facts := ownerAcceptFacts) (Frame := Hibana.OwnedWireObservation)
    ownerAcceptChecked ((Option.some_get (x := ownerAccept.ownedFacts? ownerImage admissionChoreo) (by decide)).symm) (by decide)
    (fun value => Hibana.observedResourceTag? initial.snapshot 10 = some value) dbOwnerAccepted
  have published : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders1 dbOwnerAccepted :=
    .tail accepted (.unchanged dbOwnerAccepted) publication
  have released : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders1 dbOwnerReleased :=
    .tail published (.retireAcceptedFuture dbOwnerAccepted [] [] initialDbTag) (by
      intro value present
      have ownerGone : Hibana.observedResourceTag? ownerSent 10 = none := by decide
      have workerAbsent : Hibana.observedResourceTag? initial.snapshot 10 = none := by decide
      simp only [dbHeaders1, ownerGone, workerAbsent, reduceCtorEq, or_self] at present)
  have receiving := Hibana.a_checked_owned_receive_supplies_its_transfer_header_evidence
    (facts := workerReceiveFacts) (Frame := Hibana.OwnedWireObservation)
    (by decide : observedCommit0.step.ownedCheck admissionImage admissionChoreo observedCommit0.wire 9053 = true)
    ((Option.some_get (x := observedCommit0.step.ownedFacts? admissionImage admissionChoreo) (by decide)).symm) (by decide)
    (fun value => Hibana.observedResourceTag? ownerSent 10 = some value)
    dbOwnerReleased [] observedCommit0.wire
  refine .tail released receiving.1 ?_
  intro value
  have observed : dbHeaders2 value →
      (Hibana.observedResourceTag? initial.snapshot 10 = some value ∨
        Hibana.observedResourceTag? ownerSent 10 = some value) ∨ value ∈ dbAtWorker.usable.take 1 := receiving.2 value
  intro present
  exact (observed present).elim (fun old => .inl old.symm) Or.inr

theorem observedReadyReturnKeepsThatHeaderTransferHistory :
    Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders3 dbAtWorker := by
  apply Hibana.ResourceHeaderExecution.tail observedFirstLoanHasItsCheckedHeaderTransferHistory
    (Hibana.EndpointTransfer.Step.unchanged dbAtWorker)
  intro value present
  have workerSame : Hibana.observedResourceTag? joined.snapshot 10 =
      Hibana.observedResourceTag? received.snapshot 10 :=
    Hibana.unchanged_headers_preserve_resource_tags (by decide) (by decide) (by decide) 10
  have ownerSame : Hibana.observedResourceTag? ownerBeforeReturn 10 =
      Hibana.observedResourceTag? ownerSent 10 :=
    Hibana.unchanged_headers_preserve_resource_tags (by decide) (by decide) (by decide) 10
  simpa only [dbHeaders3, dbHeaders2, workerSame, ownerSame] using present

theorem actualReadyReadHasItsCarrierValueFromTheHeaderTransferHistory :
    ∃ view, Hibana.observedActorView? received.snapshot readRow.view = some view ∧
      Hibana.observedResourceTag? received.snapshot readRow.view = some (received.snapshot.actor, view.header) ∧
      Hibana.TransferValueLocated (received.snapshot.actor, view.header) dbAtWorker := by
  obtain ⟨view, found, tagAt, custody⟩ := Hibana.a_ready_admitted_row_has_its_actual_carrier_custody
    (sample := received) (data := useData) (path := usePath) (row := readRow)
    receivedChecked ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm) (by decide) (by decide) (by decide)
    (fun value => Hibana.observedResourceTag? ownerSent 10 = some value)
    observedFirstLoanHasItsCheckedHeaderTransferHistory observedInitialHeadersIdentifyTheSoleInitialTag
  exact ⟨view, found, tagAt, custody.resolve_right (by
    simp only [dbAtWorker, dbOwnerReleased, List.mem_nil_iff, not_false_eq_true])⟩

theorem actualReadyReturnHasItsPayloadValueFromTheHeaderTransferHistory :
    ∃ view, Hibana.observedActorView? joined.snapshot resourceKey.role = some view ∧
      Hibana.observedResourceTag? joined.snapshot resourceKey.role = some (joined.snapshot.actor, view.header) ∧
      Hibana.TransferValueLocated (joined.snapshot.actor, view.header) dbAtWorker := by
  obtain ⟨view, found, tagAt, custody⟩ := Hibana.a_ready_owned_admitted_row_has_its_actual_payload_custody
    (sample := joined) (data := useData) (path := usePath) (row := returnRow)
    (binding := returnBinding) (key := resourceKey)
    joinedChecked ((Option.some_get (x := admissionImage.decodeActorOrder? admissionChoreo) (by decide)).symm) (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)
    (fun value => Hibana.observedResourceTag? ownerBeforeReturn 10 = some value)
    observedReadyReturnKeepsThatHeaderTransferHistory observedInitialHeadersIdentifyTheSoleInitialTag
  exact ⟨view, found, tagAt, custody.resolve_right (by
    simp only [dbAtWorker, dbOwnerReleased, List.mem_nil_iff, not_false_eq_true])⟩

theorem observedReturnLoanHasItsCheckedHeaderTransferHistory :
    Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders5 dbAtOwner := by
  have sealed : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders3 dbWorkerSealed :=
    .tail observedReadyReturnKeepsThatHeaderTransferHistory
      (.seal dbAtWorker [] [] receivedDbTag) (fun _ present => present)
  have accepted : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders3 dbWorkerAccepted :=
    .tail sealed (.accept dbWorkerSealed [] [] receivedDbTag observedCommit3.wire) (fun _ present => present)
  have publication := Hibana.a_checked_owned_send_supplies_its_local_publication_header_evidence
    (facts := workerAcceptFacts) (Frame := Hibana.OwnedWireObservation)
    (by decide : observedCommit3.step.ownedCheck admissionImage admissionChoreo observedCommit3.wire 9053 = true)
    ((Option.some_get (x := observedCommit3.step.ownedFacts? admissionImage admissionChoreo) (by decide)).symm) (by decide)
    (fun value => Hibana.observedResourceTag? ownerBeforeReturn 10 = some value) dbWorkerAccepted
  have published : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders4 dbWorkerAccepted :=
    .tail accepted (.unchanged dbWorkerAccepted) publication
  have released : Hibana.ResourceHeaderExecution dbHeaders0 dbStart dbHeaders4 dbWorkerReleased :=
    .tail published (.retireAcceptedFuture dbWorkerAccepted [] [] receivedDbTag) (by
      intro value present
      have workerGone : Hibana.observedResourceTag? returned.snapshot 10 = none := by decide
      have ownerAbsent : Hibana.observedResourceTag? ownerBeforeReturn 10 = none := by decide
      simp only [dbHeaders4, workerGone, ownerAbsent, reduceCtorEq, or_self] at present)
  have receiving := Hibana.a_checked_owned_receive_supplies_its_transfer_header_evidence
    (facts := ownerReturnFacts) (Frame := Hibana.OwnedWireObservation)
    ownerReturnChecked ((Option.some_get (x := ownerReturn.ownedFacts? ownerImage admissionChoreo) (by decide)).symm) (by decide)
    (fun value => Hibana.observedResourceTag? returned.snapshot 10 = some value)
    dbWorkerReleased [] observedCommit3.wire
  refine .tail released receiving.1 ?_
  intro value
  have observed : dbHeaders5 value →
      (Hibana.observedResourceTag? ownerBeforeReturn 10 = some value ∨
        Hibana.observedResourceTag? returned.snapshot 10 = some value) ∨ value ∈ dbAtOwner.usable.take 1 := receiving.2 value
  intro present
  exact (observed present).elim (fun old => .inl old.symm) Or.inr

theorem actualReturnedHeaderRealizesItsExactValue :
    Hibana.TransferValueLocated returnedDbTag dbAtOwner :=
  Hibana.an_issued_header_without_an_accepted_witness_has_its_actual_value
    observedReturnLoanHasItsCheckedHeaderTransferHistory observedInitialHeadersIdentifyTheSoleInitialTag
    (by left; decide) (by simp only [dbAtOwner, dbWorkerReleased, List.mem_nil_iff, not_false_eq_true])

theorem theHeaderModelPermitsTheNativePreemptionSchedule :
    ∃ state : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation,
      Hibana.ResourceHeaderExecution (fun value => value = initialDbTag) dbStart
        (fun value => value = initialDbTag) state ∧
      state.usable = [] ∧ state.preparing = [] ∧ state.inFlight = [observedCommit3.wire] ∧
      state.acceptedFutures = [initialDbTag] ∧ state.authorityCount = 1 :=
  Hibana.a_receiver_can_return_before_sender_header_publication
    initialDbTag receivedDbTag observedCommit0.wire observedCommit3.wire

theorem theModelPreemptionCutHasOnlyFrameAuthority :
    ∃ state : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation,
      Hibana.ResourceHeaderExecution (fun value => value = initialDbTag) dbStart
        (fun value => value = initialDbTag) state ∧
      Hibana.TransferFrameLocated observedCommit3.wire state ∧
      initialDbTag ∈ state.acceptedFutures ∧ ¬ Hibana.TransferValueLocated initialDbTag state :=
  Hibana.an_unpublished_sender_header_need_not_name_local_authority
    initialDbTag receivedDbTag observedCommit0.wire observedCommit3.wire

theorem privatePayloadAndAcceptedFramesRemainDistinctFromApplicationValues :
    dbOwnerSealed.usable = [] ∧ dbOwnerSealed.preparing = [initialDbTag] ∧
      dbOwnerAccepted.usable = [] ∧ dbOwnerAccepted.inFlight = [observedCommit0.wire] ∧
      initialDbTag ≠ receivedDbTag ∧ receivedDbTag ≠ returnedDbTag ∧ initialDbTag ≠ returnedDbTag := by decide

"#);
                let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("ActorAdmissionGenerated.lean"), source).unwrap();
            }
            drop(db);
        });
}
