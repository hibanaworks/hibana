//! A fresh DB visit preserves the completed AUTH body and DB continuation.
use super::*;
#[cfg(not(miri))]
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::time::Duration;

type Tail = g::Msg<25, ()>;
type Steps = g::Par<g::Seq<g::Roll<DbLoan>, g::Send<1, 0, Tail>>, g::Roll<AuthLoan>>;

#[test]
fn parallel_loan_reentry_preserves_the_other_body_and_completed_tail() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 32>::IMAGE_REF,
    );
    let db_service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let auth_service_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 32>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
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
            let ((owner_before, owner_after, wire), worker_commit, (), ()) =
                tokio::time::timeout(Duration::from_secs(30), async {
                    tokio::join!(
                        async {
                            owner.send::<Loan>(db).await.unwrap();
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
                            (before, after, wire)
                        },
                        async {
                            let mut db = worker.recv::<Loan>().await.unwrap();
                            let mut auth = worker.recv::<OtherLoan>().await.unwrap();
                            // Distinct resource views share one actor and actually run
                            // their two independent operation sequences concurrently.
                            let ((), ()) = tokio::join!(
                                async {
                                    db.send::<Read>(&7).await.unwrap();
                                    assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                                },
                                async {
                                    auth.send::<Read>(&9).await.unwrap();
                                    assert_eq!(auth.recv::<Value>().await.unwrap(), 10);
                                },
                            );
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
                        },
                        async {
                            let number = db_service.recv::<Read>().await.unwrap();
                            db_service.send::<Value>(&(number + 1)).await.unwrap();
                            let number = db_service.recv::<Read>().await.unwrap();
                            db_service.send::<Value>(&(number + 1)).await.unwrap();
                        },
                        async {
                            let number = auth_service.recv::<Read>().await.unwrap();
                            auth_service.send::<Value>(&(number + 1)).await.unwrap();
                        },
                    )
                })
                .await
                .expect("parallel loan reentry stopped progressing");
            let observations = [(owner_before, owner_after), worker_commit];
            for ((before, after), image) in observations.iter().zip([
                &RoleProjection::<0, Steps, 32>::IMAGE_REF,
                &RoleProjection::<1, Steps, 32>::IMAGE_REF,
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
                assert!(after.steps.iter().enumerate().all(|(index, step)| step.done
                    == (!(first..stop).contains(&index) || index == event)));
            }
            #[cfg(not(miri))]
            write_observations(&observations, &wire);
            #[cfg(miri)]
            assert_eq!(wire.1[0], 10);
        });
}

#[cfg(not(miri))]
fn write_observations(
    observations: &[(ActorSnapshot, ActorSnapshot)],
    wire: &(crate::transport::FrameHeader, std::vec::Vec<u8>),
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let images = [
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    ];
    let mut source = std::format!(
        "import Hibana.ActorReentryCommit\n\nset_option maxRecDepth 10000\n\n\
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
            super::commit::observation_source(before),
            super::commit::observation_source(after),
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

"#,
            session = wire.0.session().raw(),
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
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ActorParallelReentryGenerated.lean"), source).unwrap();
}
