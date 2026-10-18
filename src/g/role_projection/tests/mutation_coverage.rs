//! Observe actual completion writes, including an otherwise invisible rolled
//! reset/commit pair. Measurements never authorize or schedule an operation.
use super::*;
use crate::endpoint::proof_snapshot::actor_snapshot;
use crate::runtime::{SessionKitStorage, ids::SessionId};
use crate::test_support::transport::TestTransport;

#[test]
fn actual_reset_and_recommit_cannot_disappear_between_equal_snapshots() {
    type Tick = g::Msg<71, ()>;
    type Steps = g::Seq<g::Roll<g::Send<0, 2, Tick>>, g::Send<0, 1, Loan>>;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 8>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let mut slab = [0u8; 48 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            let sid = SessionId::new(9056);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let _db = rv.enter(sid, &db_program).unwrap();
            let mut peer = rv.enter(sid, &peer_program).unwrap();
            // SAFETY: the pinned actor was attached with TestTransport. Each
            // snapshot reads the counter and semantic fields under one lease.
            let initial = unsafe { actor_snapshot(&owner) };
            owner.send::<Tick>(&()).await.unwrap();
            // SAFETY: the same actor and its original view remain alive.
            let first = unsafe { actor_snapshot(&owner) };
            owner.send::<Tick>(&()).await.unwrap();
            // SAFETY: the same actor and its original view remain alive.
            let second = unsafe { actor_snapshot(&owner) };
            assert_eq!(initial.completion_mutations, 0);
            assert_eq!(first.completion_mutations, 1);
            assert_eq!(second.completion_mutations, 3);
            assert_eq!(
                (first.actor, first.run, first.counter, first.live_views,
                    &first.views, &first.steps, &first.choices),
                (second.actor, second.run, second.counter, second.live_views,
                    &second.views, &second.steps, &second.choices),
            );
            peer.recv::<Tick>().await.unwrap();
            peer.recv::<Tick>().await.unwrap();

            #[cfg(not(miri))]
            {
                use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
                let choreo = Steps::lean_source();
                let image = descriptor_source(RoleDescriptorRef::from_resident(
                    &RoleProjection::<0, Steps, 8>::IMAGE_REF,
                ));
                let source = std::format!(r#"import Hibana.ActorMutationCoverage

set_option maxRecDepth 10000
def mutationChoreo : Hibana.Choreo := {choreo}
def mutationImage : Hibana.RustDescriptorImage := {image}
def initial : Hibana.ActorProgressObservation := {initial_source}
def first : Hibana.ActorProgressObservation := {first_source}
def second : Hibana.ActorProgressObservation := {second_source}
def actualCommit : Hibana.ActorCommitRecord := {{
  step := {{ pathIndex := 0, event := 0, view := 0, before := initial, after := first }}
  wire := {{ session := 0, lane := 0, sender := 0, receiver := 0, frameLabel := 0, payload := [] }}
}}
def observedCommit : Hibana.ActorCommitMutationRecord := {{
  commit := actualCommit, before := {initial_count}, after := {first_count}
}}

theorem actualSourceCommitIsChecked : actualCommit.check mutationImage mutationChoreo 9056 = true := by decide
theorem actualCommitHasMutationCoverage :
    Hibana.actorCommitMutationHistoryCheck mutationImage mutationChoreo 9056
      initial {initial_count} first {first_count} [observedCommit] = true := by decide
theorem coveredCommitHasItsSourceHistory :
    Hibana.actorCommitHistoryCheck mutationImage mutationChoreo 9056 initial first [actualCommit] = true :=
  Hibana.checked_mutation_history_has_its_checked_source_commits actualCommitHasMutationCoverage
theorem actualOrdinarySnapshotsAreEqual : first = second := by decide
theorem theOldGapCheckAcceptsTheOmittedPair : Hibana.actorCommitGapCheck first second = true := by decide
theorem actualOmittedResetAndCommitAreRejected :
    Hibana.actorCommitMutationHistoryCheck mutationImage mutationChoreo 9056
      first {first_count} second {second_count} [] = false :=
  Hibana.an_unrecorded_mutation_interval_is_rejected mutationImage mutationChoreo 9056
    first second (by decide)
theorem actualCoveredHistoryAccountsForEveryWrite :
    {first_count} = {initial_count} + ([observedCommit] : List Hibana.ActorCommitMutationRecord).length :=
  Hibana.checked_mutation_history_accounts_for_every_observed_write actualCommitHasMutationCoverage

-- Deliberately inconsistent observations: progress was cleared but the write
-- count claims no change. These are not additional runtime executions.
def forgedCleared : Hibana.ActorProgressObservation :=
  {{ first with steps := first.steps.map fun step => {{ step with done := false }} }}
def forgedRecommit : Hibana.ActorCommitMutationRecord := {{
  commit := {{ actualCommit with step := {{ actualCommit.step with before := forgedCleared }} }}
  before := {first_count}, after := {first_count} + 1
}}
theorem aFabricatedZeroWriteClearIsRejected :
    Hibana.actorCommitMutationHistoryCheck mutationImage mutationChoreo 9056
      first {first_count} forgedCleared {first_count} [] = false := by decide
theorem aFabricatedClearBeforeARecommitIsRejected :
    Hibana.actorCommitMutationHistoryCheck mutationImage mutationChoreo 9056
      first {first_count} first ({first_count} + 1) [forgedRecommit] = false := by decide
theorem actualPendingLoanWasPendingAtEveryCoveredCommit :
    initial.doneState.done 1 = false ∧
      ∀ record ∈ [observedCommit], record.commit.step.before.doneState.done 1 = false ∧
        record.commit.step.after.doneState.done 1 = false :=
  Hibana.a_pending_final_event_was_pending_at_every_measured_commit
    actualCommitHasMutationCoverage (by decide)
theorem actualCompletedTickSurvivesAZeroWriteGap :
    first.doneState.done 0 = true ∧
      ∀ record ∈ ([] : List Hibana.ActorCommitMutationRecord),
        record.commit.step.before.doneState.done 0 = true ∧
          record.commit.step.after.doneState.done 0 = true :=
  Hibana.a_measured_history_preserves_completed_events
    (image := mutationImage) (choreo := mutationChoreo) (session := 9056)
    (first := first) (final := first)
    (firstMutation := {first_count}) (finalMutation := {first_count})
    (by decide) (by decide)
"#,
                    initial_source = super::commit::observation_source(&initial),
                    first_source = super::commit::observation_source(&first),
                    second_source = super::commit::observation_source(&second),
                    initial_count = initial.completion_mutations,
                    first_count = first.completion_mutations,
                    second_count = second.completion_mutations,
                );
                let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("ActorMutationCoverageGenerated.lean"), source).unwrap();
            }
        });
}
