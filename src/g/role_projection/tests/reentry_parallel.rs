//! A fresh DB visit preserves the completed AUTH body and DB continuation.
use super::*;
#[cfg(not(miri))]
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

type Tail = g::Msg<25, ()>;
type Steps = g::Par<g::Seq<g::Roll<DbLoan>, g::Send<1, 0, Tail>>, g::Roll<AuthLoan>>;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn parallel_loan_reentry_preserves_the_other_body_and_completed_tail()
-> core::result::Result<(), hibana::EndpointError> {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let db_service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let auth_service_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    {
        let network = TestTransport::new();
        let mut owner_slab = [0; 64 * 1024];
        let mut worker_slab = [0; 64 * 1024];
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
        let sid = SessionId::new(875);
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let db = owner_rv.enter(sid, &db_program).unwrap();
        let auth = owner_rv.enter(sid, &auth_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut db_service = worker_rv.enter(sid, &db_service_program).unwrap();
        let mut auth_service = worker_rv.enter(sid, &auth_service_program).unwrap();
        let ((owner_before, owner_after, wire, initial, first_wire), worker_commit, (), ()) =
            (async {
                async {
                    async {
                        Ok::<_, hibana::EndpointError>(
                            (async {
                                // SAFETY: owner_rv retains this pinned TestTransport actor.
                                let initial = unsafe { actor_snapshot(&owner) };
                                owner.send::<Loan>(db).await.unwrap();
                                let first_wire = network.queued_frame(1);
                                owner.send::<OtherLoan>(auth).await.unwrap();
                                let db = owner.recv::<Loan>().await.unwrap();
                                let auth = owner.recv::<OtherLoan>().await.unwrap();
                                owner.recv::<Tail>().await.unwrap();
                                // SAFETY: owner_rv attaches this TestTransport actor.
                                let before = unsafe { actor_snapshot(&owner) };
                                owner.send::<Loan>(db).await.unwrap();
                                // SAFETY: sending preserves the actor's TestTransport type.
                                let after = unsafe { actor_snapshot(&owner) };
                                // Copy the actual new-visit frame before WORKER can
                                // consume it on this current-thread executor.
                                let wire = network.queued_frame(1);
                                drop(owner.recv::<Loan>().await.unwrap());
                                drop(auth);
                                (before, after, wire, initial, first_wire)
                            })
                            .await,
                        )
                    };

                    async {
                        Ok::<_, hibana::EndpointError>(
                            (async {
                                let mut db = worker.recv::<Loan>().await.unwrap();
                                let mut auth = worker.recv::<OtherLoan>().await.unwrap();
                                // Distinct resource views share one actor and actually run
                                // their two independent operation sequences concurrently.
                                let ((), ()) = async {
                                    async {
                                        let _: () = (async {
                                            db.send::<Read>(&7).await.unwrap();
                                            assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                                        })
                                        .await;
                                        Ok::<_, hibana::EndpointError>(())
                                    };

                                    async {
                                        let _: () = (async {
                                            auth.send::<Read>(&9).await.unwrap();
                                            assert_eq!(auth.recv::<Value>().await.unwrap(), 10);
                                        })
                                        .await;
                                        Ok::<_, hibana::EndpointError>(())
                                    };
                                }
                                .await
                                .expect("all child results are data");
                                worker.send::<Loan>(db).await.unwrap();
                                worker.send::<OtherLoan>(auth).await.unwrap();
                                worker.send::<Tail>(&()).await.unwrap();
                                // SAFETY: worker_rv attaches this TestTransport actor.
                                let before = unsafe { actor_snapshot(&worker) };
                                let mut db = worker.recv::<Loan>().await.unwrap();
                                // SAFETY: receiving preserves the same concrete actor type.
                                let after = unsafe { actor_snapshot(&worker) };
                                db.send::<Read>(&11).await.unwrap();
                                assert_eq!(db.recv::<Value>().await.unwrap(), 12);
                                worker.send::<Loan>(db).await.unwrap();
                                (before, after)
                            })
                            .await,
                        )
                    };

                    async {
                        let _: () = (async {
                            let number = db_service.recv::<Read>().await.unwrap();
                            db_service.send::<Value>(&(number + 1)).await.unwrap();
                            let number = db_service.recv::<Read>().await.unwrap();
                            db_service.send::<Value>(&(number + 1)).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            let number = auth_service.recv::<Read>().await.unwrap();
                            auth_service.send::<Value>(&(number + 1)).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await
                .expect("all child results are data")
            })
            .await;
        let observations = [(owner_before, owner_after), worker_commit];
        assert!(initial.steps.iter().all(|step| !step.done));
        assert_eq!(first_wire.0.lane(), wire.0.lane());
        let auth_lane = initial
            .steps
            .iter()
            .find_map(|step| match step.action {
                LocalAction::Send { label: 2, lane, .. } => Some(lane),
                LocalAction::Send { .. }
                | LocalAction::Recv { .. }
                | LocalAction::Local { .. }
                | LocalAction::Terminate => None,
            })
            .expect("AUTH loan source lane");
        assert_ne!(auth_lane, wire.0.lane());
        for ((before, after), image) in observations.iter().zip([
            owner_program.role_image_ref(),
            worker_program.role_image_ref(),
        ]) {
            assert!(before.steps.iter().all(|step| step.done));
            let event = before
                .steps
                .iter()
                .position(|step| match step.action {
                    LocalAction::Send { label: 1, .. } => before.actor == 0,
                    LocalAction::Recv { label: 1, .. } => before.actor == 1,
                    LocalAction::Send { .. }
                    | LocalAction::Recv { .. }
                    | LocalAction::Local { .. }
                    | LocalAction::Terminate => false,
                })
                .unwrap();
            let scope = before.steps[event].reentry_scope.unwrap();
            let range = (0..usize::from(image.columns().roll_scopes.len))
                .map(|slot| image.roll_scope_row(slot).unwrap())
                .find(|row| row.scope().raw() % 8192 == scope)
                .unwrap()
                .event_row();
            let (first, stop) = (range.start(), range.end());
            assert!(before.steps.len() > stop - first);
            assert_eq!(
                after.completion_mutations,
                before.completion_mutations + (stop - first) as u64 + 2,
            );
            assert!(
                after.steps.iter().enumerate().all(|(index, step)| step.done
                    == (!(first..stop).contains(&index) || index == event))
            );
        }
        #[cfg(not(miri))]
        write_observations(
            [
                owner_program.role_image_ref(),
                worker_program.role_image_ref(),
            ],
            &observations,
            &[first_wire, wire],
            &initial,
        );
        #[cfg(miri)]
        assert_eq!(wire.1[0], 10);
    };

    Ok(())
}

#[cfg(not(miri))]
fn write_observations(
    images: [&'static RoleImageRef; 2],
    observations: &[(ActorSnapshot, ActorSnapshot)],
    wires: &[(crate::transport::FrameHeader, std::vec::Vec<u8>); 2],
    initial: &ActorSnapshot,
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let wire = &wires[1];
    let mut source = std::format!(
        "import Hibana.ActorMeasuredExecution\nimport Hibana.SendAdmission\n\nset_option maxRecDepth 10000\n\n\
         def parallelReentryChoreo : Hibana.Choreo := {}\n\n",
        Steps::lean_source(),
    );
    source.push_str(&std::format!(
        "def parallelReentryWire : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
         sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n",
        wire.0.session().raw(),
        wire.0.lane(),
        wire.0.source_role(),
        wire.0.target_role(),
        wire.0.label().raw(),
        wire.1,
    ));
    for ((before, after), image) in observations.iter().zip(images) {
        let actor = before.actor;
        assert_eq!(image.role(), actor);
        let event = before
            .steps
            .iter()
            .position(|step| match step.action {
                LocalAction::Send { label: 1, .. } => actor == 0,
                LocalAction::Recv { label: 1, .. } => actor == 1,
                LocalAction::Send { .. }
                | LocalAction::Recv { .. }
                | LocalAction::Local { .. }
                | LocalAction::Terminate => false,
            })
            .unwrap();
        let scope = before.steps[event].reentry_scope.unwrap();
        let range = (0..usize::from(image.columns().roll_scopes.len))
            .map(|slot| image.roll_scope_row(slot).unwrap())
            .find(|row| row.scope().raw() % 8192 == scope)
            .unwrap()
            .event_row();
        let (first, stop) = (range.start(), range.end());
        let wrong_scope = (0..usize::from(image.columns().roll_scopes.len))
            .map(|slot| image.roll_scope_row(slot).unwrap().scope().raw() % 8192)
            .find(|candidate| *candidate != scope)
            .unwrap();
        let name = std::format!("parallelReentryActor{actor}");
        source.push_str(&std::format!(
            "def parallelReentryImage{actor} : Hibana.RustDescriptorImage := {}\n\n\
             def {name} : Hibana.ActorReentryCommitObservation := {{\n  \
             previousPath := 0, nextPath := 0, scope := {scope}, event := {event}, view := {actor},\n  \
             before := {},\n  after := {}\n}}\n\n\
             theorem {name}Accepted :\n  \
             {name}.check parallelReentryImage{actor} parallelReentryChoreo = true := by\n  decide\n\n\
             theorem {name}SourceOrdered :\n  \
             {name}.SourceOrdered parallelReentryImage{actor} parallelReentryChoreo :=\n  \
             Hibana.accepted_body_reentry_restores_source_progress {name}Accepted\n\n\
             theorem {name}MissingResetRejected :\n  \
             ({{ {name} with after := {{ {name}.after with steps := {name}.before.steps }} }} : \
             Hibana.ActorReentryCommitObservation).check parallelReentryImage{actor} parallelReentryChoreo = false := by\n  decide\n\n\
             theorem {name}ClearedSiblingRejected :\n  \
             ({{ {name} with after := {{ {name}.after with steps := \
             {name}.after.steps.mapIdx (fun event step =>\n    \
             if {first} ≤ event && event < {stop} then step else {{ step with done := false }}) }} }} : \
             Hibana.ActorReentryCommitObservation).check parallelReentryImage{actor} parallelReentryChoreo = false := by\n  decide\n\n\
             theorem {name}WrongScopeRejected :\n  \
             ({{ {name} with scope := {wrong_scope} }} : \
             Hibana.ActorReentryCommitObservation).check parallelReentryImage{actor} parallelReentryChoreo = false := by\n  decide\n\n\
             theorem {name}WrongViewCountRejected :\n  \
             ({{ {name} with after := {{ {name}.after with liveViews := {name}.after.liveViews + 1 }} }} : \
             Hibana.ActorReentryCommitObservation).check parallelReentryImage{actor} parallelReentryChoreo = false := by\n  decide\n\n",
            descriptor_source(RoleDescriptorRef::from_resident(image)),
            crate::endpoint::proof_snapshot::observation_source(before),
            crate::endpoint::proof_snapshot::observation_source(after),
        ));
        source.push_str(&std::format!(
            r#"
def {name}Prepared : Hibana.DecodedRollRow × Hibana.ActorCommitObservation :=
  ({name}.prepared? parallelReentryImage{actor} parallelReentryChoreo).get (by decide)

theorem {name}OwnedAccepted :
    {name}.ownedCheck parallelReentryImage{actor} parallelReentryChoreo parallelReentryWire {session} = true := by decide
theorem {name}PreparedOwned :
    {name}Prepared.2.ownedCheck parallelReentryImage{actor} parallelReentryChoreo parallelReentryWire {session} = true := by decide
theorem {name}ValueTagsSurviveReset :
    {name}Prepared.2.before.actor = {name}.before.actor ∧
      {name}Prepared.2.before.run = {name}.before.run ∧
      {name}Prepared.2.before.views = {name}.before.views :=
  Hibana.prepared_body_reentry_preserves_value_identity
    ((Option.some_get (x := {name}.prepared? parallelReentryImage{actor} parallelReentryChoreo) (by decide)).symm)
theorem {name}WrongEnvelopeRejected :
    {name}.ownedCheck parallelReentryImage{actor} parallelReentryChoreo
      ({{ parallelReentryWire with payload := 11 :: parallelReentryWire.payload.drop 1 }}) {session} = false := by decide
theorem {name}WrongSenderRejected :
    {name}.ownedCheck parallelReentryImage{actor} parallelReentryChoreo
      ({{ parallelReentryWire with sender := parallelReentryWire.sender + 1 }}) {session} = false := by decide
theorem {name}WrongSessionRejected :
    {name}.ownedCheck parallelReentryImage{actor} parallelReentryChoreo
      ({{ parallelReentryWire with session := parallelReentryWire.session + 1 }}) {session} = false := by decide

theorem {name}ActualResetWritesChecked :
    Hibana.actorReentryMutationCheck {name} parallelReentryWire
      parallelReentryImage{actor} parallelReentryChoreo {session} {before_count} {after_count} = true := by decide
theorem {name}ActualResetWritesAndSource :
    {name}.SourceOrdered parallelReentryImage{actor} parallelReentryChoreo ∧
      ∃ roll step, {name}.prepared? parallelReentryImage{actor} parallelReentryChoreo = some (roll, step) ∧
        ({{ step, wire := parallelReentryWire }} : Hibana.ActorCommitRecord).check
          parallelReentryImage{actor} parallelReentryChoreo {session} = true ∧
        {after_count} = {before_count} + roll.eventLength +
          Hibana.rollCompletionWriteCount parallelReentryImage{actor} roll + 1 :=
  Hibana.a_measured_reentry_has_its_source_reset_and_exact_writes {name}ActualResetWritesChecked
theorem {name}OmittedResetWritesRejected :
    Hibana.actorReentryMutationCheck {name} parallelReentryWire
      parallelReentryImage{actor} parallelReentryChoreo {session} {before_count} ({before_count} + 1) = false := by decide

def {name}MeasuredResetRows : List Nat :=
  (List.range {name}Prepared.1.eventLength).map ({name}Prepared.1.eventStart + ·)

theorem {name}ActualMeasuredFreshHistory :
    Hibana.ActorMeasuredHistory parallelReentryImage{actor} parallelReentryChoreo {session}
      ({name}.before, {before_count}) ({name}.after, {after_count})
      {name}MeasuredResetRows
        (Hibana.rollCompletionWriteCount parallelReentryImage{actor} {name}Prepared.1 + 1) :=
  .tail .refl (.reentry {name}ActualResetWritesChecked
    ((Option.some_get (x := {name}.prepared? parallelReentryImage{actor} parallelReentryChoreo) (by decide)).symm))

theorem {name}MeasuredHistoryAccountsForAllWrites :
    {after_count} = {before_count} + {name}MeasuredResetRows.length +
      (Hibana.rollCompletionWriteCount parallelReentryImage{actor} {name}Prepared.1 + 1) :=
  Hibana.a_measured_actor_history_accounts_for_every_mark_and_clear {name}ActualMeasuredFreshHistory

theorem {name}RequiredProgressSurvivesEveryIndependentSuffix
    {{final : Hibana.ActorProgressObservation × Nat}} {{cleared : List Nat}} {{commits event : Nat}}
    (suffix : Hibana.ActorMeasuredHistory parallelReentryImage{actor} parallelReentryChoreo {session}
      ({name}.after, {after_count}) final cleared commits)
    (untouched : event ∉ {name}MeasuredResetRows ++ cleared)
    (completed : {name}.before.doneState.done event = true) :
    final.1.doneState.done event = true :=
  Hibana.a_measured_actor_history_preserves_completion_outside_its_resets
    (Hibana.measured_actor_histories_compose_at_the_same_boundary {name}ActualMeasuredFreshHistory suffix)
    untouched completed

"#,
            session = wire.0.session().raw(),
            before_count = before.completion_mutations,
            after_count = after.completion_mutations,
        ));
    }
    source.push_str(
        r#"
def parallelReentrySendFacts : Hibana.OwnedCommitFacts :=
  (parallelReentryActor0Prepared.2.ownedFacts? parallelReentryImage0 parallelReentryChoreo).get (by decide)
def parallelReentryRecvFacts : Hibana.OwnedCommitFacts :=
  (parallelReentryActor1Prepared.2.ownedFacts? parallelReentryImage1 parallelReentryChoreo).get (by decide)

theorem parallelReentryFrameNamesTheSameContinuation :
    parallelReentrySendFacts.resource = parallelReentryRecvFacts.resource ∧
      parallelReentrySendFacts.effIndex = parallelReentryRecvFacts.effIndex :=
  Hibana.checked_owned_frame_has_the_same_source_at_send_and_receive
    parallelReentryActor0PreparedOwned parallelReentryActor1PreparedOwned
    ((Option.some_get (x := parallelReentryActor0Prepared.2.ownedFacts? parallelReentryImage0 parallelReentryChoreo) (by decide)).symm)
    ((Option.some_get (x := parallelReentryActor1Prepared.2.ownedFacts? parallelReentryImage1 parallelReentryChoreo) (by decide)).symm)
    rfl

theorem parallelReentryReceiverPublishesItsActualActor :
    parallelReentryActor1Prepared.2.after.actor = parallelReentryRecvFacts.binding.receiverActor :=
  Hibana.checked_owned_receive_publishes_its_projected_actor
    parallelReentryActor1PreparedOwned
    ((Option.some_get (x := parallelReentryActor1Prepared.2.ownedFacts? parallelReentryImage1 parallelReentryChoreo) (by decide)).symm)
    (by decide)

"#,
    );
    source.push_str(&std::format!(
        "\ndef parallelInitial : Hibana.ActorProgressObservation := {}\n\n\
         def parallelFirstWire : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, \
         receiver := {}, frameLabel := {}, payload := {:?} }}\n\n",
        crate::endpoint::proof_snapshot::observation_source(initial), wires[0].0.session().raw(), wires[0].0.lane(),
        wires[0].0.source_role(), wires[0].0.target_role(), wires[0].0.label().raw(), wires[0].1,
    ));
    source.push_str(r#"
def parallelFirstAdmission : Hibana.SourceSendAdmission := {
  image := parallelReentryImage0, base := parallelInitial, pathIndex := 0,
  event := parallelReentryActor0.event, wire := parallelFirstWire }
def parallelFreshAdmission : Hibana.SourceSendAdmission := {
  image := parallelReentryImage0,
  base := ((Hibana.prepareObservedActorBody? parallelReentryImage0 parallelReentryChoreo
    parallelReentryActor0.before 0 0 parallelReentryActor0.scope parallelReentryActor0.event).get (by decide)).2,
  pathIndex := 0, event := parallelReentryActor0.event, wire := parallelReentryWire }

theorem parallelFirstAcceptanceIsSourceChecked : parallelFirstAdmission.check parallelReentryChoreo 875 = true := by decide
theorem parallelFreshAcceptanceIsSourceChecked : parallelFreshAdmission.check parallelReentryChoreo 875 = true := by decide

def parallelFirstSource := (parallelFirstAdmission.ownedFacts? parallelReentryChoreo).get (by decide)
def parallelFreshSource := (parallelFreshAdmission.ownedFacts? parallelReentryChoreo).get (by decide)
def parallelChannel : Hibana.TransportChannel := {
  session := 875, generation := 0, lane := parallelFirstWire.lane,
  sender := parallelFirstWire.sender, receiver := parallelFirstWire.receiver }
def parallelCarrier0 := Hibana.TransportState.initial parallelChannel
def parallelGhost0 := Hibana.ElasticAdmissionHistory.initial parallelChannel
def parallelCarrier1 := (parallelCarrier0.send ⟨parallelFirstWire.frameLabel, by decide⟩).get (by decide)
def parallelGhost1 := parallelGhost0.appendOccurrence parallelFirstSource.effIndex
def parallelCarrierAfterFirst := { parallelCarrier1 with delivered := 1 }
def parallelGhostAfterFirst := { parallelGhost1 with delivered := 1 }
def parallelCarrier2 := (parallelCarrierAfterFirst.send ⟨parallelReentryWire.frameLabel, by decide⟩).get (by decide)
def parallelGhost2 := parallelGhostAfterFirst.appendOccurrence parallelFreshSource.effIndex
def parallelAcceptedRecords := [(parallelFirstAdmission, parallelFirstSource.effIndex), (parallelFreshAdmission, parallelFreshSource.effIndex)]

theorem parallelActualAcceptancesKeepTheirFIFOInstances :
    Hibana.SendAdmissionTrace parallelReentryChoreo 875 parallelCarrier2 parallelGhost2 parallelAcceptedRecords := by
  apply Hibana.SendAdmissionTrace.accept
    (choreo := parallelReentryChoreo) (session := 875)
    (transport := parallelCarrierAfterFirst) (history := parallelGhostAfterFirst)
    (records := [(parallelFirstAdmission, parallelFirstSource.effIndex)])
    (sample := parallelFreshAdmission) (eventId := parallelFreshSource.effIndex)
    (label := ⟨parallelReentryWire.frameLabel, by decide⟩)
  · apply Hibana.SendAdmissionTrace.receive
      (transport := parallelCarrier1) (history := parallelGhost1)
      (records := [(parallelFirstAdmission, parallelFirstSource.effIndex)])
      (frame := (parallelCarrier1.frames[0]?).get (by decide))
      (key := (parallelGhost1.accepted[0]?).get (by decide))
    · exact .accept (transport := parallelCarrier0) (sent := parallelCarrier1)
        (history := parallelGhost0) (records := [])
        (sample := parallelFirstAdmission) (eventId := parallelFirstSource.effIndex)
        (label := ⟨parallelFirstWire.frameLabel, by decide⟩)
        (.initial parallelChannel) parallelFirstAcceptanceIsSourceChecked
        (by decide) (by decide) ⟨rfl, rfl, rfl, rfl, rfl⟩
    · decide
    · rfl
  · exact parallelFreshAcceptanceIsSourceChecked
  · decide
  · decide
  · exact ⟨rfl, rfl, rfl, rfl, rfl⟩

theorem parallelFreshBytesBindTheActualReceiverWithoutSenderPublication :
    parallelFreshSource.resource = parallelReentryRecvFacts.resource ∧
      parallelFreshSource.effIndex = parallelReentryRecvFacts.effIndex :=
  Hibana.accepted_owned_source_bytes_identify_the_checked_receiver
    parallelFreshAcceptanceIsSourceChecked
    ((Option.some_get (x := parallelFreshAdmission.ownedFacts? parallelReentryChoreo) (by decide)).symm)
    parallelReentryActor1PreparedOwned
    ((Option.some_get (x := parallelReentryActor1Prepared.2.ownedFacts? parallelReentryImage1 parallelReentryChoreo) (by decide)).symm)
    rfl

theorem parallelRepeatedEnvelopeHasDistinctFIFOInstances :
    parallelFirstWire.payload = parallelReentryWire.payload ∧
      parallelGhost2.accepted = [⟨parallelFreshSource.effIndex, 0⟩, ⟨parallelFreshSource.effIndex, 1⟩] ∧
      parallelGhost2.delivered = 1 := by decide

theorem parallelActualReceiveHasItsFreshAcceptedOccurrence :
    ∃ sample eventId key following,
      parallelAcceptedRecords[parallelCarrier2.delivered]? = some (sample, eventId) ∧
      parallelGhost2.accepted[parallelCarrier2.delivered]? = some key ∧
      sample.check parallelReentryChoreo 875 = true ∧ sample.effIndex? = some eventId ∧
      sample.wire.identifies ((parallelCarrier2.frames[1]?).get (by decide)) ∧ key.globalId = eventId ∧
      ((parallelCarrier2.frames[1]?).get (by decide)).sequence = parallelGhost2.delivered ∧
      key.iteration < parallelGhost2.nextIteration key.globalId ∧
      parallelGhost2.receive? = some (key, following) ∧
      Hibana.SendAdmissionTrace parallelReentryChoreo 875
        ({ parallelCarrier2 with delivered := 2 }) following parallelAcceptedRecords :=
  Hibana.a_fifo_receive_has_its_actual_accepted_source_occurrence
    parallelActualAcceptancesKeepTheirFIFOInstances (by decide)
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ActorParallelReentryGenerated.lean"), source).unwrap();
}

#[cfg(not(miri))]
#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_parent_reset_counts_the_actual_credited_child_writes()
-> core::result::Result<(), hibana::EndpointError> {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    use std::fmt::Write;

    type Head = g::Msg<0, ()>;
    type Reply = g::Msg<1, ()>;
    type Child = g::Msg<2, ()>;
    type ChildReply = g::Msg<3, ()>;
    type Body = g::Roll<
        g::Par<
            g::Seq<g::Send<0, 1, Head>, g::Send<1, 0, Reply>>,
            g::Roll<g::Seq<g::Send<0, 1, Child>, g::Send<1, 0, ChildReply>>>,
        >,
    >;
    let global = g::Program::<Body>::new();
    let sender_program = crate::runtime::program::project::<0>(&global);
    let receiver_program = crate::runtime::program::project::<1>(&global);
    let image = receiver_program.role_image_ref();
    assert_eq!(image.footprint().borrow_epoch_count, 2);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1975);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    let mut source = std::format!(
        "import Hibana.ActorMutationCoverage\nopen Hibana\nset_option maxRecDepth 10000\n\n\
         def protocol : Choreo := {}\ndef image : RustDescriptorImage := {}\n",
        Body::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(image)),
    );
    sender.send::<Head>(&()).await?;
    receiver.recv::<Head>().await?;
    receiver.send::<Reply>(&()).await?;
    sender.recv::<Reply>().await?;
    sender.send::<Child>(&()).await?;
    receiver.recv::<Child>().await?;
    receiver.send::<ChildReply>(&()).await?;
    sender.recv::<ChildReply>().await?;

    for (visit, credited) in ["uncredited", "preserved"].into_iter().zip([false, true]) {
        if credited {
            // Start the child again after the parent has finished its visit.
            // Its reply is still owed when the independent parent head arrives.
            sender.send::<Child>(&()).await?;
            receiver.recv::<Child>().await?;
        }
        sender.send::<Head>(&()).await?;
        let (header, payload) = network.queued_frame(1);
        // SAFETY: both samples use the receiver's pinned TestTransport actor.
        let before = unsafe { actor_snapshot(&receiver) };
        receiver.recv::<Head>().await?;
        let after = unsafe { actor_snapshot(&receiver) };
        let parent = before.steps[0].reentry_scope.unwrap();
        let child = (0..image.footprint().roll_scope_count)
            .map(|slot| image.roll_scope_row(slot).unwrap())
            .find(|row| row.event_row().len() == 2)
            .unwrap()
            .scope()
            .local_ordinal();
        assert_eq!(
            before.credited_rolls,
            if credited {
                std::vec![child]
            } else {
                std::vec![]
            }
        );
        assert!(after.credited_rolls.is_empty());
        assert_eq!(
            before
                .steps
                .iter()
                .map(|r| r.done)
                .collect::<std::vec::Vec<_>>(),
            [true, true, true, !credited]
        );
        assert_eq!(
            after
                .steps
                .iter()
                .map(|r| r.done)
                .collect::<std::vec::Vec<_>>(),
            [true, false, credited, false]
        );
        // Each reset scope writes its epoch, credit and retained bit. The
        // independent child costs only one credit write when preserved.
        assert_eq!(
            after.completion_mutations - before.completion_mutations,
            if credited {
                2 + 3 + 1 + 1
            } else {
                4 + 2 * 3 + 1
            }
        );
        writeln!(source, "
namespace {visit}
def before : ActorProgressObservation := {}
def after : ActorProgressObservation := {}
def retainedBefore : List Nat := {:?}
def retainedAfter : List Nat := {:?}
def creditBefore : List Nat := {:?}
def creditAfter : List Nat := {:?}
def firstWrite : Nat := {}
def lastWrite : Nat := {}
def parent : Nat := {}
def child : Nat := {}
def wire : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}
def sample : ActorReentryCommitObservation := {{ previousPath := 0, nextPath := 0, scope := parent, event := 0, view := 1, before, after }}
def base := (prepareObservedActorBody? image protocol before 0 0 parent 0 creditBefore).get (by decide)
def order := (image.decodeActorOrder? protocol).get (by decide)
def path := (order.normalized.paths[0]?).get (by decide)
def kept := sourceIndependentRetainedChildren order path (image.decodeRollRows?.get (by decide)) creditBefore base.1 0
def checkedStep : ActorCommitRecord := {{ step := {{ pathIndex := 0, event := 0, view := 1, before := base.2, after }}, wire }}
theorem actualCommit : checkedStep.check image protocol {} = true := by decide
theorem measuredWrites : lastWrite = firstWrite + rollResetWriteCount image base.1 kept + 1 := by decide
end {visit}",
            crate::endpoint::proof_snapshot::observation_source(&before),
            crate::endpoint::proof_snapshot::observation_source(&after),
            before.retained_rolls, after.retained_rolls, before.credited_rolls, after.credited_rolls,
            before.completion_mutations, after.completion_mutations, parent, child,
            header.session().raw(), header.lane(), header.source_role(), header.target_role(),
            header.label().raw(), payload, sid.raw()).unwrap();
        receiver.send::<Reply>(&()).await?;
        sender.recv::<Reply>().await?;
        if !credited {
            sender.send::<Child>(&()).await?;
            receiver.recv::<Child>().await?;
        }
        receiver.send::<ChildReply>(&()).await?;
        sender.recv::<ChildReply>().await?;
    }
    assert!(network.queue_is_empty());
    source.push_str(r#"
theorem zeroCreditMeasured :
    uncredited.creditBefore = [] ∧ uncredited.creditAfter = [] ∧ uncredited.kept.isEmpty ∧
    actorReentryMutationCheck uncredited.sample uncredited.wire image protocol 1975
      uncredited.firstWrite uncredited.lastWrite = true := by decide

theorem retainedChildMeasured :
    preserved.creditBefore = [preserved.child] ∧ preserved.creditAfter = [] ∧
    preserved.kept.map (·.scope) = [preserved.child] ∧
    preserved.retainedBefore.contains preserved.child ∧ preserved.retainedAfter.contains preserved.child ∧
    preserved.base.2.progress = [false, false, true, false] ∧
    preserved.after.progress = [true, false, true, false] ∧
    preserved.lastWrite = preserved.firstWrite + 2 + 3 + 1 + 1 := by decide
"#);
    if let Some(output) = std::env::var_os("HIBANA_REENTRY_WRITE_PROOF") {
        std::fs::write(output, source).unwrap();
    }
    Ok(())
}
