//! A legal earlier body can refresh a resource after a later receipt. Source
//! occurrence order must not be mistaken for the current value's provenance.
use super::*;
#[cfg(not(miri))]
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::{
    endpoint::proof_snapshot::{actor_admissions, actor_snapshot, view_tag},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

type OutsideLoan = g::Msg<6, Endpoint<'static, 10>>;
type AfterRead = g::Msg<30, u32>;
type Steps = g::Seq<
    g::Roll<DbLoan>,
    g::Seq<
        g::Send<0, 2, OutsideLoan>,
        g::Seq<g::Send<2, 0, OutsideLoan>, g::Send<10, 3, AfterRead>>,
    >,
>;

#[test]
fn a_reentered_earlier_body_refreshes_the_value_after_a_later_source_receipt() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut peer_slab = [0; 32 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut peer_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let peer_rv = peer_storage
                .init()
                .rendezvous(&mut peer_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9058);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = peer_rv.enter(sid, &worker_program).unwrap();
            let mut peer = peer_rv.enter(sid, &peer_program).unwrap();
            let mut service = peer_rv.enter(sid, &service_program).unwrap();
            let (observed, loop_wires, (outside_wire, ())) =
                tokio::time::timeout(core::time::Duration::from_secs(240), async {
                    tokio::join!(
                        async {
                            // SAFETY: OWNER and DB use owner_rv's pinned actor.
                            let initial = unsafe { actor_snapshot(&owner) };
                            let initial_admitted = unsafe { actor_admissions(&owner) };
                            owner.send::<Loan>(db).await.unwrap();
                            // SAFETY: every copied cut belongs to owner_rv's pinned actor.
                            let first_send = unsafe { actor_snapshot(&owner) };
                            let first_send_wire = network.queued_frame(1);
                            let db = owner.recv::<Loan>().await.unwrap();
                            let first_receipt = unsafe { actor_snapshot(&owner) };
                            owner.send::<OutsideLoan>(db).await.unwrap();
                            // SAFETY: OWNER retains this pinned TestTransport actor.
                            let outside_before = unsafe { actor_snapshot(&owner) };
                            let outside_send_wire = network.queued_frame(2);
                            let outside_before_admitted = unsafe { actor_admissions(&owner) };
                            let db = owner.recv::<OutsideLoan>().await.unwrap();
                            // SAFETY: OWNER retains this pinned TestTransport actor.
                            let later_receipt = unsafe { actor_snapshot(&owner) };
                            let outside_after_admitted = unsafe { actor_admissions(&owner) };
                            let old_tag = view_tag(&db);
                            assert!(unsafe { old_tag.matches_header() });

                            owner.send::<Loan>(db).await.unwrap();
                            // SAFETY: the same actor remains pinned after the new send.
                            let fresh_before = unsafe { actor_snapshot(&owner) };
                            let fresh_send_wire = network.queued_frame(1);
                            let fresh_before_admitted = unsafe { actor_admissions(&owner) };
                            let mut db = owner.recv::<Loan>().await.unwrap();
                            // SAFETY: the same actor remains pinned after the fresh body.
                            let refreshed = unsafe { actor_snapshot(&owner) };
                            assert!(!unsafe { old_tag.matches_header() });
                            assert!(
                                later_receipt
                                    .steps
                                    .iter()
                                    .map(|step| step.done)
                                    .eq(refreshed.steps.iter().map(|step| step.done))
                            );
                            assert!(refreshed.counter > later_receipt.counter);
                            assert!(
                                refreshed.completion_mutations > later_receipt.completion_mutations
                            );
                            // SAFETY: both observations use the same pinned actor
                            // and its concrete TestTransport without advancing it.
                            let admitted = unsafe { actor_admissions(&owner) };
                            let call = unsafe { view_tag(&db).actor_call_cut::<TestTransport>() };
                            assert!(call.lease_available);
                            assert_eq!(call.fault, None);
                            db.send::<AfterRead>(&91).await.unwrap();
                            (
                                [
                                    initial,
                                    first_send,
                                    first_receipt,
                                    outside_before,
                                    later_receipt,
                                    fresh_before,
                                    refreshed,
                                ],
                                [
                                    initial_admitted,
                                    outside_before_admitted,
                                    outside_after_admitted,
                                    fresh_before_admitted,
                                    admitted,
                                ],
                                [first_send_wire, outside_send_wire, fresh_send_wire],
                            )
                        },
                        async {
                            let mut wires = std::vec::Vec::new();
                            for number in [17, 23] {
                                let mut db = worker.recv::<Loan>().await.unwrap();
                                db.send::<Read>(&number).await.unwrap();
                                assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                                worker.send::<Loan>(db).await.unwrap();
                                // Copy the accepted frame before OWNER can consume
                                // it on this cooperative current-thread executor.
                                wires.push(network.queued_frame(0));
                            }
                            wires
                        },
                        async {
                            tokio::join!(
                                async {
                                    let db = peer.recv::<OutsideLoan>().await.unwrap();
                                    peer.send::<OutsideLoan>(db).await.unwrap();
                                    network.queued_frame(0)
                                },
                                async {
                                    for number in [17, 23] {
                                        assert_eq!(service.recv::<Read>().await.unwrap(), number);
                                        service.send::<Value>(&(number + 1)).await.unwrap();
                                    }
                                    assert_eq!(service.recv::<AfterRead>().await.unwrap(), 91);
                                },
                            )
                        },
                    )
                })
                .await
                .expect("a legal closed body could not refresh its resource");
            assert!(network.queue_is_empty());
            assert_eq!(observed.0[6].counter, observed.0[4].counter + 2);
            assert_eq!(loop_wires.len(), 2);
            assert_eq!(loop_wires[1].1[0], 10);
            assert_eq!(outside_wire.1[0], 10);
            let read = observed.0[6]
                .steps
                .iter()
                .position(|step| matches!(step.action, LocalAction::Send { label: 30, .. }))
                .unwrap();
            assert!(observed.1[4][read]);
            #[cfg(not(miri))]
            write_observations(
                &observed.0,
                &observed.1,
                &outside_wire,
                &loop_wires,
                &observed.2,
            );
        });
}

#[cfg(not(miri))]
fn write_observations(
    snapshots: &[ActorSnapshot; 7],
    admitted: &[std::vec::Vec<bool>; 5],
    outside_wire: &(crate::transport::FrameHeader, std::vec::Vec<u8>),
    loop_wires: &[(crate::transport::FrameHeader, std::vec::Vec<u8>)],
    sent: &[(crate::transport::FrameHeader, std::vec::Vec<u8>); 3],
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let mut source = std::format!(
        "import Hibana.ResourceHeaderOrigin\nimport Hibana.SourceResourceContinuation\n\
         import Hibana.ActorOccurrenceHistory\n\nset_option maxRecDepth 10000\n\n\
         def refreshChoreo : Hibana.Choreo := {}\n\n\
         def refreshBodyChoreo : Hibana.Choreo := {}\n\n\
         def refreshImage : Hibana.RustDescriptorImage := {}\n\n",
        Steps::lean_source(),
        DbLoan::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            &RoleProjection::<0, Steps, 16>::IMAGE_REF,
        )),
    );
    for (name, snapshot) in [
        "initial",
        "firstSend",
        "firstReceipt",
        "outsideBefore",
        "outsideAfter",
        "freshBefore",
        "current",
    ]
    .into_iter()
    .zip(snapshots)
    {
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorProgressObservation := {}\n\n",
            super::commit::observation_source(snapshot),
        ));
    }
    for (name, label, before, after, (header, payload)) in [
        ("outside", 6, &snapshots[3], &snapshots[4], outside_wire),
        ("fresh", 1, &snapshots[5], &snapshots[6], &loop_wires[1]),
    ] {
        let event = before
            .steps
            .iter()
            .position(|step| matches!(step.action, LocalAction::Recv { label: found, .. } if found == label))
            .unwrap();
        source.push_str(&std::format!(
            "def {name}Receipt : Hibana.ActorCommitObservation := {{ pathIndex := 0, \
             event := {event}, view := 0, before := {}, after := {} }}\n\n\
             def {name}Wire : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {payload:?} }}\n\n",
            super::commit::observation_source(before),
            super::commit::observation_source(after),
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
            header.label().raw(),
        ));
    }
    source.push_str(&std::format!(
        "def refreshAdmissions : List Bool := {:?}\n\n",
        admitted[4],
    ));
    for (name, candidates) in [
        "initial",
        "outsideBefore",
        "outsideAfter",
        "freshBefore",
        "current",
    ]
    .into_iter()
    .zip(admitted)
    {
        source.push_str(&std::format!(
            "def {name}Candidates : Hibana.ActorAdmissionObservation := {{ snapshot := {name}, \
             pathIndex := 0, admitted := {candidates:?} }}\n\n\
             theorem {name}CandidateDomainIsChecked : \
             {name}Candidates.candidateCheck refreshImage refreshChoreo 0 = true := by decide\n\n",
        ));
    }
    source.push_str(r#"
open Hibana

def refreshData : DecodedActorOrder := (refreshImage.decodeActorOrder? refreshChoreo).get (by decide)
def refreshPath := (refreshData.normalized.paths[0]?).get (by decide)
def readRow := (refreshData.rows.find? fun row => row.view == 10).get (by decide)
def outsideFacts := (outsideReceipt.ownedFacts? refreshImage refreshChoreo).get (by decide)
def freshFacts := (freshReceipt.ownedFacts? refreshImage refreshChoreo).get (by decide)
def refreshBody := refreshBodyChoreo.ownershipSource?.get (by decide)
def freshRollRun := (Hibana.EndpointDelegation.elaborate refreshData.source.resources
  (Hibana.EndpointDelegation.initialOwners refreshData.source.resources)
  refreshPath.selections (.roll refreshBody) 0).get (by decide)

theorem actualSourceBeginsWithThisClosedBody : ∃ tail, refreshData.source = .seq (.roll refreshBody) tail := by
  exact ⟨_, rfl⟩
theorem actualRepeatedBodyRestoresItsResourceOwner :
    Hibana.EndpointDelegation.instantiate refreshData.normalized.issuance
      (freshRollRun.owners freshFacts.resource) =
    Hibana.EndpointDelegation.instantiate refreshData.normalized.issuance
      (Hibana.EndpointDelegation.initialOwners refreshData.source.resources freshFacts.resource) := by
  exact an_accepted_roll_preserves_each_instantiated_resource_owner
    ((Option.some_get (x := Hibana.EndpointDelegation.elaborate refreshData.source.resources
      (Hibana.EndpointDelegation.initialOwners refreshData.source.resources)
      refreshPath.selections (.roll refreshBody) 0) (by decide)).symm)
    (by decide) (by decide)

theorem outsideActualReceiptIsChecked :
    outsideReceipt.ownedCheck refreshImage refreshChoreo outsideWire 9058 = true := by decide
theorem freshActualReceiptIsChecked :
    freshReceipt.ownedCheck refreshImage refreshChoreo freshWire 9058 = true := by decide
theorem actualReceiptsNameTheSameResource : outsideFacts.resource = freshFacts.resource := by decide
theorem latestActualReceiptHasAnEarlierSourceEvent : freshFacts.effIndex < outsideFacts.effIndex := by decide
theorem actualDoneBitsAreTheSameAfterFreshReceipt : outsideAfter.doneState.done = current.doneState.done := by
  funext event
  rfl
theorem actualCurrentHeaderComesFromFreshReceipt :
    observedResourceTag? current 10 = observedResourceTag? freshReceipt.after 10 := rfl
theorem actualOldReceiptHeaderIsNoLongerCurrent :
    observedResourceTag? current 10 ≠ observedResourceTag? outsideAfter 10 := by decide
theorem actualSourceContinuationIsReady : sourceRowReady refreshData refreshPath current readRow = true := by decide
theorem actualContinuationIsAdmitted : refreshAdmissions[readRow.eventId]? = some true := by decide
theorem actualCurrentHeaderIsFresh :
    (resourceNonceCut current 10).issued = some current.counter ∧ outsideAfter.counter < current.counter := by decide

def bodyEntry := (refreshData.rows[0]?).get (by decide)
def bodyRoll := (((refreshChoreo.canonicalActorRollRows? refreshImage.role).get (by decide))[0]?).get (by decide)
def bodyBase := (prepareObservedActorBody? refreshImage refreshChoreo current 0 0 bodyRoll.scope bodyEntry.eventId).get (by decide) |>.2

theorem actualCompletedBodyHasFreshRawAdmission :
    current.doneState.done bodyEntry.eventId = true ∧ currentCandidates.admitted[bodyEntry.eventId]? = some true := by decide
theorem actualCurrentEntryIsNotReadyWithoutPreparation :
    sourceRowReady refreshData refreshPath current bodyEntry = false := by decide
theorem actualFreshEntryUsesSourcePreparation :
    ActorSourceCandidateBase currentCandidates refreshImage refreshChoreo 0 refreshPath bodyEntry bodyBase := by
  exact .fresh (roll := bodyRoll)
    (rolls := (refreshChoreo.canonicalActorRollRows? refreshImage.role).get (by decide))
    (by decide) (by decide) (by decide)
theorem actualFreshSourceReadinessGivesAdmission : currentCandidates.admitted[bodyEntry.eventId]? = some true := by
  have admitted : sourceRowReady refreshData refreshPath bodyBase bodyEntry = true ∧
      currentCandidates.admitted[bodyEntry.eventId]? = some true := by
    refine a_structurally_ready_source_point_has_actual_candidate_admission
      (observed := fun point => match point with
        | .committed id => (refreshData.activeRows refreshPath).any fun row =>
            decide (row.actionId = id) && bodyBase.doneState.done row.eventId
        | .accepted _ => false)
      currentCandidateDomainIsChecked
      ((Option.some_get (x := refreshImage.decodeActorOrder? refreshChoreo) (by decide)).symm)
      ((Option.some_get (x := refreshData.normalized.paths[0]?) (by decide)).symm)
      (by decide) actualFreshEntryUsesSourcePreparation ?_ (by decide) ?_
    · intro prior member
      have rows : (refreshData.activeRows refreshPath).all (fun prior => decide (
          ((refreshData.activeRows refreshPath).any fun row =>
            decide (row.actionId = prior.actionId) && bodyBase.doneState.done row.eventId) =
          bodyBase.doneState.done prior.eventId)) = true := by decide
      exact of_decide_eq_true (List.all_eq_true.mp rows prior member)
    · intro before edge
      have earlier := source_point_dependency_increases_rank edge
      have first : bodyEntry.actionId.entryPoint.rank = 0 := by decide
      rw [first] at earlier
      omega
  exact admitted.2
theorem actualLaterSourcePointHasRealAdmission :
    sourceRowReady refreshData refreshPath current readRow = true ∧ currentCandidates.admitted[readRow.eventId]? = some true := by
  refine a_structurally_ready_source_point_has_actual_candidate_admission
    (observed := fun point => decide (point.rank < readRow.actionId.entryPoint.rank))
    currentCandidateDomainIsChecked
    ((Option.some_get (x := refreshImage.decodeActorOrder? refreshChoreo) (by decide)).symm)
    ((Option.some_get (x := refreshData.normalized.paths[0]?) (by decide)).symm)
    (by decide) (.current (by decide)) ?_ (by decide) ?_
  · intro prior member
    have rows : (refreshData.activeRows refreshPath).all (fun prior =>
        decide (decide ((.committed prior.actionId : SourcePoint).rank < readRow.actionId.entryPoint.rank) =
          current.doneState.done prior.eventId)) = true := by decide
    exact of_decide_eq_true (List.all_eq_true.mp rows prior member)
  · intro before edge
    exact decide_eq_true (source_point_dependency_increases_rank edge)

theorem missingFreshAdmissionIsRejected :
    ActorAdmissionObservation.candidateCheck
      ({ currentCandidates with admitted := currentCandidates.admitted.set bodyEntry.eventId false })
      refreshImage refreshChoreo 0 = false := by decide
theorem fabricatedBlockedAdmissionIsRejected :
    ActorAdmissionObservation.candidateCheck
      ({ freshBeforeCandidates with admitted := freshBeforeCandidates.admitted.set readRow.eventId true })
      refreshImage refreshChoreo 0 = false := by decide
theorem omittedCandidateRowIsRejected :
    ActorAdmissionObservation.candidateCheck
      ({ currentCandidates with admitted := currentCandidates.admitted.drop 1 })
      refreshImage refreshChoreo 0 = false := by decide

def initialTag := (observedResourceTag? initial 10).get (by decide)
def currentTag := (observedResourceTag? current 10).get (by decide)
def actualIssued (tag : Nat × Nat × Nat × Nat) : Prop := observedResourceTag? current 10 = some tag

/-- Complete carrier/header and originating-call histories remain explicit
    correspondence contracts. A retained done bit does not supply them. -/
theorem currentSourceReadyCutUsesActualReceiptProvenance
    {Frame : Type} {first : (Nat × Nat × Nat × Nat) → Prop}
    {state : Hibana.EndpointTransfer.Observation (Nat × Nat × Nat × Nat) Frame}
    (headers : ResourceHeaderExecution first (Hibana.EndpointTransfer.initial initialTag) actualIssued state)
    (transfers : Hibana.EndpointTransfer.Execution (Hibana.EndpointTransfer.initial initialTag) state)
    (initialHeaders : ∀ tag, first tag ↔ tag = initialTag)
    (calls : ∀ before after, Hibana.EndpointTransfer.Execution (Hibana.EndpointTransfer.initial initialTag) before →
      Hibana.EndpointTransfer.Step .accept before after → after.acceptedFutures.head? = some currentTag →
      Hibana.EndpointTransfer.Execution after state →
      AcceptedSendCallEvidence currentTag.2.2.2 (resourceNonceCut current 10) False False) :
    sourceRowReady refreshData refreshPath current readRow = true ∧
      Hibana.TransferValueLocated currentTag state ∧
      ∃ beforeIssued before afterIssued after,
        ResourceHeaderExecution first (Hibana.EndpointTransfer.initial initialTag) beforeIssued before ∧
        Hibana.EndpointTransfer.Step .receive before after ∧
        ResourceHeaderStepEvidence .receive before after beforeIssued afterIssued ∧
        currentTag ∈ after.usable.take 1 ∧
        HeaderSuffixWithoutReceipt currentTag afterIssued after actualIssued state := by
  refine ⟨actualSourceContinuationIsReady,
    a_fresh_live_public_header_has_actual_custody_and_receive_origin
      headers transfers initialHeaders ?_ ?_ ?_ (fun impossible => impossible) (fun impossible => impossible) calls⟩
  · exact (Option.some_get (x := observedResourceTag? current 10) (by decide)).symm
  · decide
  · decide
"#);
    // Serialize every owner commit in its actual order. These observations
    // never select an operation or supply a value to the running localsides.
    let cut_names = [
        "initial",
        "firstSend",
        "firstReceipt",
        "outsideBefore",
        "outsideAfter",
        "freshBefore",
        "current",
    ];
    let mut history = std::string::String::from(".refl");
    for (index, label, sending, wire) in [
        (0, 1, true, &sent[0]),
        (1, 1, false, &loop_wires[0]),
        (2, 6, true, &sent[1]),
        (3, 6, false, outside_wire),
        (4, 1, true, &sent[2]),
        (5, 1, false, &loop_wires[1]),
    ] {
        let before = &snapshots[index];
        let after = &snapshots[index + 1];
        let event = before
            .steps
            .iter()
            .position(|step| match step.action {
                LocalAction::Send { label: found, .. } => sending && found == label,
                LocalAction::Recv { label: found, .. } => !sending && found == label,
                LocalAction::Local { .. } | LocalAction::Terminate => false,
            })
            .unwrap();
        let before_name = cut_names[index];
        let after_name = cut_names[index + 1];
        let (header, payload) = wire;
        source.push_str(&std::format!(
            "def mixedWire{index} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {payload:?} }}\n\n",
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
            header.label().raw(),
        ));
        let step = if index == 4 {
            let scope = before.steps[event].reentry_scope.unwrap();
            source.push_str(&std::format!(
                "def mixedFresh : Hibana.ActorReentryCommitObservation := {{ previousPath := 0, nextPath := 0, \
                 scope := {scope}, event := {event}, view := 0, before := {before_name}, after := {after_name} }}\n\n\
                 def mixedFreshPrepared := (mixedFresh.prepared? refreshImage refreshChoreo).get (by decide)\n\n\
                 def mixedCleared := (List.range mixedFreshPrepared.1.eventLength).map (mixedFreshPrepared.1.eventStart + ·)\n\n\
                 theorem mixedFreshWritesChecked : Hibana.actorReentryMutationCheck mixedFresh mixedWire4 \
                 refreshImage refreshChoreo {} {} {} = true := by decide\n\n\
                 theorem mixedFreshOmittedClearsRejected : Hibana.actorReentryMutationCheck mixedFresh mixedWire4 \
                 refreshImage refreshChoreo {} {} ({} + 1) = false := by decide\n\n",
                header.session().raw(), before.completion_mutations, after.completion_mutations,
                header.session().raw(), before.completion_mutations, before.completion_mutations,
            ));
            std::string::String::from(
                ".reentry mixedFreshWritesChecked ((Option.some_get (x := mixedFresh.prepared? refreshImage refreshChoreo) (by decide)).symm)",
            )
        } else {
            source.push_str(&std::format!(
                "def mixedRecord{index} : Hibana.ActorCommitRecord := {{ wire := mixedWire{index}, \
                 step := {{ pathIndex := 0, event := {event}, view := 0, before := {before_name}, after := {after_name} }} }}\n\n\
                 theorem mixedRecord{index}Checked : mixedRecord{index}.check refreshImage refreshChoreo {} = true := by decide\n\n",
                header.session().raw(),
            ));
            std::format!(
                ".commit (record := {{ commit := mixedRecord{index}, before := {}, after := {} }}) \
                mixedRecord{index}Checked (by decide)",
                before.completion_mutations,
                after.completion_mutations
            )
        };
        history = std::format!(".tail ({history}) ({step})");
    }
    source.push_str(&std::format!(
        "theorem actualCompleteOwnerMixedHistory : Hibana.ActorMeasuredHistory refreshImage refreshChoreo {} \
         (initial, {}) (current, {}) mixedCleared 7 :=\n  {history}\n\n",
        outside_wire.0.session().raw(), snapshots[0].completion_mutations, snapshots[6].completion_mutations,
    ));
    source.push_str(r#"
theorem everyCurrentReadPrerequisiteHasItsActualMixedCommitOrigin
    (event : Nat) (required : event ∈ refreshData.sourcePredecessors refreshPath readRow.eventId) :
    ∃ before after prior marked later priorCommits laterCommits record,
      ActorMeasuredHistory refreshImage refreshChoreo 9058 (initial, 0) before prior priorCommits ∧
      ActorMeasuredCommit refreshImage refreshChoreo 9058 before after marked record ∧
      ActorMeasuredHistory refreshImage refreshChoreo 9058 after (current, 9) later laterCommits ∧
      record.step.event = event ∧ event ∉ later ∧
      record.check refreshImage refreshChoreo 9058 = true ∧ record.step.SourceOrdered refreshImage refreshChoreo := by
  have allInitial : (refreshData.sourcePredecessors refreshPath readRow.eventId).all
      (fun required => !initial.doneState.done required) = true := by decide
  have initiallyPending : initial.doneState.done event = false := by
    have pending := List.all_eq_true.mp allInitial event required
    cases doneAt : initial.doneState.done event with
    | false => rfl
    | true => simp only [doneAt, Bool.not_true, Bool.false_eq_true] at pending
  exact a_ready_candidate_predecessor_has_its_mixed_history_origin
    (sample := currentCandidates) (previousPath := 0) actualCompleteOwnerMixedHistory initiallyPending
    (.current (by decide)) required actualSourceContinuationIsReady

theorem actualMixedHistoryCountsEveryClearAndMark :
    9 = 0 + mixedCleared.length + 7 :=
  a_measured_actor_history_accounts_for_every_mark_and_clear actualCompleteOwnerMixedHistory
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ReceiptRefreshGenerated.lean"), source).unwrap();
}
