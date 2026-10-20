//! Qualify an actual last-receipt interval, including an older handoff and an
//! independent operation completing while the resource's I/O remains pending.
use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[test]
fn a_last_receipt_keeps_past_and_future_handoffs_outside_pending_parallel_work() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<1, 0, Loan>,
            g::Seq<
                g::Send<0, 1, Loan>,
                g::Seq<
                    g::Par<g::Send<10, 3, Read>, g::Send<1, 4, Value>>,
                    g::Seq<g::Send<10, 3, Read>, g::Send<1, 0, Loan>>,
                >,
            >,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let reader_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let peer_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut worker_slab = [0; 32 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut worker_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let worker_rv = worker_storage
                .init()
                .rendezvous(&mut worker_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9057);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let mut reader = worker_rv.enter(sid, &reader_program).unwrap();
            let mut peer = worker_rv.enter(sid, &peer_program).unwrap();
            // SAFETY: these attached Endpoints retain their TestTransport actors.
            let owner_initial = unsafe { actor_snapshot(&owner) };
            // SAFETY: WORKER retains this pinned actor throughout the fixture.
            let initial = unsafe { actor_snapshot(&worker) };
            assert_eq!(owner_initial.completion_mutations, 0);
            owner.send::<Loan>(db).await.unwrap();
            #[cfg(not(miri))]
            let first_wire = network.queued_frame(1);
            let db = worker.recv::<Loan>().await.unwrap();
            // SAFETY: the same actor remains pinned after the completed call.
            let first_received = unsafe { actor_snapshot(&worker) };
            worker.send::<Loan>(db).await.unwrap();
            #[cfg(not(miri))]
            let old_return_wire = network.queued_frame(0);
            // SAFETY: WORKER's own view keeps this actor alive after moving DB.
            let first_sent = unsafe { actor_snapshot(&worker) };
            let db = owner.recv::<Loan>().await.unwrap();
            owner.send::<Loan>(db).await.unwrap();
            #[cfg(not(miri))]
            let current_wire = network.queued_frame(1);
            let mut db = worker.recv::<Loan>().await.unwrap();
            // SAFETY: the same actor has just committed its second receipt.
            let current = unsafe { actor_snapshot(&worker) };
            assert!(current.steps[1].done);
            assert!(current.steps[2].done);
            assert!(!current.steps[3].done);
            assert!(!current.steps[4].done);
            assert!(!current.steps[5].done);

            // This pauses only carrier readiness. The two Endpoint operations
            // remain in the same projected par; no application flag or owner
            // table decides which operation is permitted.
            network.pause_send(10);
            let (read_done, value_done) = tokio::join!(biased;
                async {
                    db.send::<Read>(&23).await.unwrap();
                    // SAFETY: DB retains this actor after releasing its poll lease.
                    unsafe { actor_snapshot(&db) }
                },
                async {
                    assert!(network.queue_is_empty());
                    worker.send::<Value>(&29).await.unwrap();
                    // SAFETY: the independent send has released the actor lease;
                    // DB's send is still pending and cannot have committed.
                    let cut = unsafe { actor_snapshot(&worker) };
                    assert!(!cut.steps[3].done);
                    assert!(cut.steps[4].done);
                    network.resume_send();
                    cut
                },
            );
            #[cfg(not(miri))]
            let read_wire = network.queued_frame(3);
            #[cfg(not(miri))]
            let value_wire = network.queued_frame(4);
            assert_eq!(reader.recv::<Read>().await.unwrap(), 23);
            assert_eq!(peer.recv::<Value>().await.unwrap(), 29);
            db.send::<Read>(&31).await.unwrap();
            // SAFETY: this view retains the same actor after the post-join read.
            let second_done = unsafe { actor_snapshot(&db) };
            #[cfg(not(miri))]
            let second_read_wire = network.queued_frame(3);
            assert_eq!(reader.recv::<Read>().await.unwrap(), 31);
            worker.send::<Loan>(db).await.unwrap();
            #[cfg(not(miri))]
            let return_wire = network.queued_frame(0);
            // SAFETY: WORKER remains attached after DB was moved out.
            let returned = unsafe { actor_snapshot(&worker) };
            let _db = owner.recv::<Loan>().await.unwrap();
            assert!(network.queue_is_empty());
            assert_eq!(initial.completion_mutations, 0);
            assert_eq!(first_received.completion_mutations, 1);
            assert_eq!(first_sent.completion_mutations, 2);
            assert_eq!(current.completion_mutations, 3);
            assert_eq!(value_done.completion_mutations, 4);
            assert_eq!(read_done.completion_mutations, 5);
            assert_eq!(second_done.completion_mutations, 6);
            assert_eq!(returned.completion_mutations, 7);

            #[cfg(not(miri))]
            {
                use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
                let choreo = Steps::lean_source();
                let image = descriptor_source(RoleDescriptorRef::from_resident(
                    &RoleProjection::<1, Steps, 16>::IMAGE_REF,
                ));
                let owner_image = descriptor_source(RoleDescriptorRef::from_resident(
                    &RoleProjection::<0, Steps, 16>::IMAGE_REF,
                ));
                let mut source = std::format!(
                    "import Hibana.MeasuredResourceAvailability\n\nset_option maxRecDepth 10000\n\
                     def intervalChoreo : Hibana.Choreo := {choreo}\n\
                     def intervalImage : Hibana.RustDescriptorImage := {image}\n\
                     def intervalOwnerImage : Hibana.RustDescriptorImage := {owner_image}\n"
                );
                for (name, snapshot) in [
                    ("initial", &initial), ("firstReceived", &first_received),
                    ("firstSent", &first_sent), ("current", &current),
                    ("cut", &value_done), ("joined", &read_done), ("afterRead", &second_done),
                    ("returned", &returned), ("ownerInitial", &owner_initial),
                ] {
                    source.push_str(&std::format!(
                        "def {name} : Hibana.ActorProgressObservation := {}\n\
                         def {name}Count : Nat := {}\n",
                        super::commit::observation_source(snapshot), snapshot.completion_mutations,
                    ));
                }
                // The Read future started before Value, but its commit happens
                // after Value. Its actual before cut is the copied Value cut,
                // not the snapshot taken before the asynchronous call began.
                for (index, (event, view, before, after, (header, payload))) in [
                    (0, 1, &initial, &first_received, first_wire),
                    (1, 1, &first_received, &first_sent, old_return_wire),
                    (2, 1, &first_sent, &current, current_wire),
                    (4, 1, &current, &value_done, value_wire),
                    (3, 10, &value_done, &read_done, read_wire),
                    (5, 10, &read_done, &second_done, second_read_wire),
                    (6, 1, &second_done, &returned, return_wire),
                ].into_iter().enumerate() {
                    source.push_str(&std::format!(
                        "def actual{index} : Hibana.ActorCommitMutationRecord := {{\n\
                         commit := {{ step := {{ pathIndex := 0, event := {event}, view := {view}, \
                         before := {}, after := {} }}, wire := {{ session := {}, lane := {}, \
                         sender := {}, receiver := {}, frameLabel := {}, payload := {payload:?} }} }}\n\
                         before := {}, after := {}\n}}\n",
                        super::commit::observation_source(before), super::commit::observation_source(after),
                        header.session().raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(),
                        before.completion_mutations, after.completion_mutations,
                    ));
                }
                source.push_str(r#"
open Hibana Hibana.EndpointDelegation
def intervalData : DecodedActorOrder := (intervalImage.decodeActorOrder? intervalChoreo).get (by decide)
def intervalPath : NormalizedPath := (intervalData.normalized.paths[0]?).get (by decide)
def intervalRun : Elaboration := (elaborate intervalData.source.resources
  (initialOwners intervalData.source.resources) intervalPath.selections intervalData.source 0).get (by decide)
def intervalIssuance := intervalData.normalized.issuance
def resource : EndpointKey := { component := 0, role := 10 }
def receiptRow := (intervalData.rows[2]?).get (by decide)
def readRow := (intervalData.rows[5]?).get (by decide)
def previousReturn := (intervalData.rows[1]?).get (by decide)
def nextReturn := (intervalData.rows[6]?).get (by decide)
def readBinding := ((intervalPath.bindings.find? fun entry => entry.1 == readRow.actionId.event).get (by decide)).2
def previousBinding := ((intervalPath.bindings.find? fun entry => entry.1 == previousReturn.actionId.event).get (by decide)).2
def nextBinding := ((intervalPath.bindings.find? fun entry => entry.1 == nextReturn.actionId.event).get (by decide)).2
def readSymbolic := (intervalRun.bindings[5]?).get (by decide) |>.2
def previousSymbolic := (intervalRun.bindings[2]?).get (by decide) |>.2
def returnSymbolic := (intervalRun.bindings[6]?).get (by decide) |>.2

theorem actualActorRowsAreChecked : intervalImage.actorRowsCheck intervalChoreo = true := by decide
theorem actualCompleteHistoryIsMeasured : actorCommitMutationHistoryCheck intervalImage intervalChoreo 9057
    initial initialCount returned returnedCount [actual0, actual1, actual2, actual3, actual4, actual5, actual6] = true := by decide
theorem actualIndependentWorkCompletedWhileReadWasPending :
    cut.doneState.done 3 = false ∧ cut.doneState.done 4 = true := by decide
theorem actualTargetWasNotReadyAtTheReceipt : sourceRowReady intervalData intervalPath current readRow = false := by decide
theorem actualTargetBecomesReadyAfterTheParallelWork : sourceRowReady intervalData intervalPath joined readRow = true := by decide
theorem actualOwnReturnBecomesReadyAfterTheTarget : sourceRowReady intervalData intervalPath afterRead nextReturn = true := by decide
theorem actualReceiptClosesItsSelectedSourcePrefix :
    current.doneState.done receiptRow.eventId = true ∧
      current.doneState.PrefixClosed (intervalData.sourcePredecessors intervalPath) [receiptRow.eventId] := by
  have ordered := accepted_actor_commit_matches_source_progress
    (Bool.and_eq_true_iff.mp
      (show actual2.commit.step.ownedCheck intervalImage intervalChoreo actual2.commit.wire 9057 = true from by decide)).1
  obtain ⟨_, data, path, row, binding, decoded, pathAt, rowAt, eventAt, _, _, _, _, done, closed, _⟩ := ordered
  have dataSame : data = intervalData := Option.some.inj (decoded.symm.trans
    ((Option.some_get (x := intervalImage.decodeActorOrder? intervalChoreo) (by decide)).symm))
  subst data
  have pathSame : path = intervalPath := Option.some.inj (pathAt.symm.trans
    ((Option.some_get (x := intervalData.normalized.paths[0]?) (by decide)).symm))
  subst path
  refine ⟨done, ?_⟩
  intro event member completed before beforeMember
  apply closed event ?_ completed before beforeMember
  simpa only [receiptRow, actual2] using (List.mem_cons.mpr (.inl (List.mem_singleton.mp member)))

theorem actualMeasuredMiddleInterval :
    actorCommitMutationHistoryCheck intervalImage intervalChoreo 9057 current currentCount cut cutCount [actual3] = true ∧
      actorCommitMutationHistoryCheck intervalImage intervalChoreo 9057 cut cutCount joined joinedCount [actual4] = true := by decide

theorem everyDistinctHandoffIsBlockedAtThisActualCut
    (transfer : DecodedActorRow) (loan : Binding)
    (member : transfer ∈ intervalData.activeRows intervalPath)
    (located : intervalPath.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (payload : loan.message.payload = .endpoint resource)
    (sending : transfer.actionId.side = .send)
    (different : transfer.actionId.event ≠ readRow.actionId.event) :
    sourceRowReady intervalData intervalPath cut transfer = false := by
  apply a_measured_last_receipt_interval_excludes_a_distinct_handoff
    (target := readRow) (targetBinding := readBinding) (receipt := receiptRow)
    (issuance := intervalIssuance) (run := intervalRun) (index := 5)
    (targetSymbolic := readSymbolic) (previous := previousSymbolic)
    (earlierRows := intervalRun.bindings.take 2) (laterRows := (intervalRun.bindings.drop 3).take 2)
    (first := current) (cut := cut) (final := joined) (prior := [actual3]) (later := [actual4])
    actualActorRowsAreChecked
    ((Option.some_get (x := intervalImage.decodeActorOrder? intervalChoreo) (by decide)).symm)
    (by decide) (by decide) member (by decide) located payload
    (Or.inl (by decide)) sending (by decide) different
    ((Option.some_get (x := elaborate intervalData.source.resources
      (initialOwners intervalData.source.resources) intervalPath.selections intervalData.source 0) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)
    actualReceiptClosesItsSelectedSourcePrefix.1 actualReceiptClosesItsSelectedSourcePrefix.2
    actualMeasuredMiddleInterval.1 actualMeasuredMiddleInterval.2 (by decide)

theorem actualEarlierReturnCannotBeRepeatedAtThisCut : sourceRowReady intervalData intervalPath cut previousReturn = false :=
  everyDistinctHandoffIsBlockedAtThisActualCut previousReturn previousBinding (by decide) (by decide) (by decide) (by decide) (by decide)
theorem actualLaterReturnCannotTakeThePendingResource : sourceRowReady intervalData intervalPath cut nextReturn = false :=
  everyDistinctHandoffIsBlockedAtThisActualCut nextReturn nextBinding (by decide) (by decide) (by decide) (by decide) (by decide)

theorem actualReadyReadHasItsLastWireCheckedReceipt :
    SourceReadyReceiptEvidence intervalImage intervalChoreo 9057 intervalData intervalPath readRow readBinding resource
      [actual0.commit, actual1.commit, actual2.commit, actual3.commit, actual4.commit] joined :=
  source_ready_resource_has_initial_issuer_or_current_wire_receipt
    (first := initial) (final := joined) (records := [actual0.commit, actual1.commit, actual2.commit, actual3.commit, actual4.commit])
    actualActorRowsAreChecked
    ((Option.some_get (x := intervalImage.decodeActorOrder? intervalChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (Or.inl (by decide)) (by decide)
    (checked_mutation_history_has_its_checked_source_commits
      (firstMutation := initialCount) (finalMutation := joinedCount)
      (records := [actual0, actual1, actual2, actual3, actual4]) (by decide))
    (by
      intro receipt member _ _ _ _
      have allPending : (intervalData.activeRows intervalPath).all
          (fun row => !initial.doneState.done row.eventId) = true := by decide
      simpa using List.all_eq_true.mp allPending receipt member)

theorem actualReadHasItsReceiptIntervalOrigin :
    SourceIntervalOrigin intervalData intervalPath readRow readBinding resource current := by
  refine ⟨intervalIssuance, intervalRun, 5, readSymbolic,
    (Option.some_get (x := elaborate intervalData.source.resources
      (initialOwners intervalData.source.resources) intervalPath.selections intervalData.source 0)
      (by decide)).symm,
    (by decide), (by decide), (by decide), .inr ?_⟩
  exact ⟨receiptRow, previousSymbolic, intervalRun.bindings.take 2,
    (intervalRun.bindings.drop 3).take 2, (by decide), (by decide), (by decide),
    (by decide), (by decide), actualReceiptClosesItsSelectedSourcePrefix⟩

theorem actualReturnHasItsReceiptIntervalOrigin :
    SourceIntervalOrigin intervalData intervalPath nextReturn nextBinding resource current := by
  refine ⟨intervalIssuance, intervalRun, 6, returnSymbolic,
    (Option.some_get (x := elaborate intervalData.source.resources
      (initialOwners intervalData.source.resources) intervalPath.selections intervalData.source 0)
      (by decide)).symm,
    (by decide), (by decide), (by decide), .inr ?_⟩
  exact ⟨receiptRow, previousSymbolic, intervalRun.bindings.take 2,
    (intervalRun.bindings.drop 3).take 3, (by decide), (by decide), (by decide),
    (by decide), (by decide), actualReceiptClosesItsSelectedSourcePrefix⟩

def actualReceiptFacts : OwnedCommitFacts :=
  (actual2.commit.step.ownedFacts? intervalImage intervalChoreo).get (by decide)

-- The carrier and complete transfer suffix are universally quantified here.
-- Their correspondence to this binding remains an implementation contract;
-- they are not fabricated from the copied endpoint header or an assumed value.
theorem actualCheckedReceiptKeepsItsValueUntilTheMeasuredReadBoundary
    {carrier : CarrierSemantics} {abstraction : carrier.State → TransportBoundaryState}
    {Fairness : Prop} (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {currentCarrier offered committed : carrier.State} {frame : TransportFrame}
    {before : TransportState}
    {state : EndpointTransfer.Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {rest : List TransportFrame} (initialView : Nat × Nat × Nat × Nat)
    (execution : EndpointTransfer.Execution (EndpointTransfer.initial initialView) state)
    (currentExact : abstraction currentCarrier = .idle before) (wellFormed : before.WellFormed)
    (received : carrier.pollReceive currentCarrier = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (identity : actual2.commit.wire.identifies frame) (queued : state.inFlight = frame :: rest) :
    current.actor = actualReceiptFacts.binding.receiverActor ∧
      actual2.commit.step.SourceOrdered intervalImage intervalChoreo ∧
      ∃ next after, EndpointTransfer.Step .receive state next ∧
        TransferValueLocated (current.actor, actualReceiptFacts.afterView.header) next ∧
        abstraction committed = .idle after ∧ before.delivered < after.delivered ∧
        ∀ final, EndpointTransfer.Execution next final →
          (∀ prior following action, EndpointTransfer.Execution next prior →
            TransferValueLocated (current.actor, actualReceiptFacts.afterView.header) prior →
            EndpointTransfer.Step action prior following → EndpointTransfer.Execution following final →
            MeasuredResourceBoundaryEvidence intervalImage intervalChoreo 9057 intervalData intervalPath resource
              current currentCount joined joinedCount action) →
          TransferValueLocated (current.actor, actualReceiptFacts.afterView.header) final ∨
          ∃ prior following action, EndpointTransfer.Execution next prior ∧
            EndpointTransfer.Step action prior following ∧
            TransferValueLocated (current.actor, actualReceiptFacts.afterView.header) prior ∧
            EndpointTransfer.Execution following final ∧
            SourceMessageBoundaryOutcome intervalImage intervalChoreo intervalData intervalPath resource readRow action ∧
            (action = .accept → ∃ acceptedFrame, TransferFrameLocated acceptedFrame following) := by
  exact a_checked_receipt_retains_its_value_until_a_measured_source_boundary
    (sample := actual2.commit.step) (session := 9057) (facts := actualReceiptFacts)
    (data := intervalData) (path := intervalPath) (target := readRow) (binding := readBinding)
    (firstCount := currentCount) (finalCount := joinedCount) (finalProgress := joined)
    (by decide)
    (Option.some_get (x := actual2.commit.step.ownedFacts? intervalImage intervalChoreo) (by decide)).symm
    (by decide)
    ((Option.some_get (x := intervalImage.decodeActorOrder? intervalChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (Or.inl (by decide)) (by decide)
    actualReadHasItsReceiptIntervalOrigin actualTargetBecomesReadyAfterTheParallelWork
    profile initialView execution currentExact wellFormed received resolved identity queued

theorem actualOrdinaryReadCanOnlyRetireWithFault {action : EndpointTransfer.Action}
    (outcome : SourceMessageBoundaryOutcome intervalImage intervalChoreo intervalData intervalPath resource readRow action) :
    (action = .abandonPreparation ∨ action = .dropValue) ∧
      ∃ sample : ViewRetirementObservation, sample.check intervalImage intervalChoreo = true ∧
        sample.view = resource.role ∧ sample.after.fault ≠ none :=
  the_measured_boundary_of_an_ordinary_use_is_a_fault (binding := readBinding) (by decide) (by decide) outcome

theorem actualSendingReturnBoundaryKeepsItsExactSourceAction {action : EndpointTransfer.Action}
    (outcome : SourceMessageBoundaryOutcome intervalImage intervalChoreo intervalData intervalPath resource nextReturn action) :
    SourceHandoffBoundaryOutcome intervalImage intervalChoreo intervalData intervalPath resource nextReturn action :=
  the_measured_boundary_of_a_sending_target_is_its_exact_handoff_or_fault (by decide) outcome

def initialData : DecodedActorOrder := (intervalOwnerImage.decodeActorOrder? intervalChoreo).get (by decide)
def initialPath : NormalizedPath := (initialData.normalized.paths[0]?).get (by decide)
def initialRun : Elaboration := (elaborate initialData.source.resources
  (initialOwners initialData.source.resources) initialPath.selections initialData.source 0).get (by decide)
def firstLoan := (initialData.rows[0]?).get (by decide)
def laterLoan := (initialData.rows[2]?).get (by decide)
def firstLoanBinding := ((initialPath.bindings.find? fun entry => entry.1 == firstLoan.actionId.event).get (by decide)).2
def laterLoanBinding := ((initialPath.bindings.find? fun entry => entry.1 == laterLoan.actionId.event).get (by decide)).2
def firstLoanSymbolic := (initialRun.bindings[0]?).get (by decide) |>.2

theorem actualInitialOwnerAgreesWithTheSourceIssuer :
    ownerInitial.viewIssued resource.role = true ∧
      instantiate initialData.normalized.issuance ((initialOwners initialData.source.resources) resource) =
        some initialData.actor := by decide

theorem actualInitialOwnerExcludesTheLaterLoan : sourceRowReady initialData initialPath ownerInitial laterLoan = false := by
  apply a_measured_initial_resource_interval_excludes_a_distinct_handoff
    (session := 9057)
    (target := firstLoan) (targetBinding := firstLoanBinding) (loan := laterLoanBinding)
    (key := resource) (issuance := initialData.normalized.issuance) (run := initialRun) (index := 0)
    (targetSymbolic := firstLoanSymbolic) (cut := ownerInitial) (final := ownerInitial)
    (cutCount := ownerInitialCount) (finalCount := ownerInitialCount) (later := [])
    (by decide)
    ((Option.some_get (x := intervalOwnerImage.decodeActorOrder? intervalChoreo) (by decide)).symm)
    (by decide) (by decide) (by decide) (by decide) (by decide)
    (Or.inr (Or.inr (by decide))) (by decide)
    ((Option.some_get (x := elaborate initialData.source.resources
      (initialOwners initialData.source.resources) initialPath.selections initialData.source 0) (by decide)).symm)
    (by decide) (by decide) (by decide) (by simp) (by decide) (by decide)
"#);
                let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("SourceHandoffIntervalGenerated.lean"), source).unwrap();
            }
        });
}
