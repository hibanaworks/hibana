import Hibana.ActorMeasuredExecution
import Hibana.ActorCandidateAdmission

namespace Hibana
open EndpointDelegation

/-- The actual commit at one measured boundary. For a fresh body the record's
before state is its prepared base, while the measured before state precedes
the reset. This erased witness adds neither a runtime visit nor a scheduler. -/
inductive ActorMeasuredCommit (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    ActorProgressObservation × Nat → ActorProgressObservation × Nat →
      List Nat → ActorCommitRecord → Prop where
  | ordinary {record : ActorCommitMutationRecord}
      (checked : record.commit.check image choreo session = true)
      (written : record.after = record.before + 1) :
      ActorMeasuredCommit image choreo session
        (record.commit.step.before, record.before) (record.commit.step.after, record.after)
        [] record.commit
  | fresh {sample : ActorReentryCommitObservation} {wire : OwnedWireObservation}
      {before after : Nat} {roll : DecodedRollRow} {step : ActorCommitObservation}
      (checked : actorReentryMutationCheck sample wire image choreo session before after = true)
      (prepared : sample.prepared? image choreo = some (roll, step)) :
      ActorMeasuredCommit image choreo session (sample.before, before) (sample.after, after)
        ((List.range roll.eventLength).map (roll.eventStart + ·)) { step, wire }

theorem a_measured_commit_retains_its_checked_source_and_wire
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
    (commit : ActorMeasuredCommit image choreo session before after cleared record) :
    record.check image choreo session = true ∧ record.step.SourceOrdered image choreo := by
  have checked : record.check image choreo session = true := by
    cases commit with
    | ordinary checked written => exact checked
    | fresh checked prepared =>
      obtain ⟨_, actualRoll, actualStep, actualAt, valid, _⟩ := a_measured_reentry_has_its_source_reset_and_exact_writes checked
      have same := congrArg Prod.snd (Option.some.inj (actualAt.symm.trans prepared))
      cases same
      exact valid
  exact ⟨checked, accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)⟩

private theorem a_measured_step_either_marks_the_row_or_preserves_its_origin
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (step : ActorMeasuredStep image choreo session before after cleared commits)
    (completed : after.1.doneState.done event = true) :
    (∃ record, ActorMeasuredCommit image choreo session before after cleared record ∧
      record.step.event = event) ∨
    (before.1.doneState.done event = true ∧ event ∉ cleared) := by
  cases step with
  | quiet checked =>
    have progressAt := beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).1
    exact .inr ⟨by simpa only [ActorProgressObservation.doneState, progressAt] using completed, by simp⟩
  | @commit record checked written =>
    by_cases same : record.commit.step.event = event
    · exact .inl ⟨record.commit, .ordinary checked written, same⟩
    · obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, unchanged, _⟩ :=
        accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
      exact .inr ⟨(unchanged event (Ne.symm same)).symm.trans completed, by simp⟩
  | @reentry sample wire first final roll operation checked prepared =>
    by_cases same : operation.event = event
    · exact .inl ⟨{ step := operation, wire }, .fresh checked prepared, same⟩
    · obtain ⟨ordered, _⟩ := a_measured_reentry_has_its_source_reset_and_exact_writes checked
      obtain ⟨actual, step, rows, actualAt, _, _, reset, stepOrdered⟩ := ordered
      have identity := Option.some.inj (actualAt.symm.trans prepared)
      have rollSame := congrArg Prod.fst identity
      have stepSame := congrArg Prod.snd identity
      change actual = roll at rollSame
      change step = operation at stepSame
      subst actual
      subst step
      have actualOperation := prepared_body_reentry_keeps_its_actual_operation prepared
      obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, unchanged, _⟩ := stepOrdered
      have old : (if roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength
          then false else sample.before.doneState.done event) = true := by
        rw [← reset event, ← unchanged event (Ne.symm same), actualOperation.2.1]
        exact completed
      split at old
      next inside => contradiction
      next outside =>
        right
        refine ⟨old, ?_⟩
        intro member
        obtain ⟨offset, bound, atEvent⟩ := List.mem_map.mp member
        have limit := List.mem_range.mp bound
        have range : roll.eventStart ≤ event ∧ event < roll.eventStart + roll.eventLength := by
          rw [← atEvent]
          exact ⟨Nat.le_add_right _ _, Nat.add_lt_add_left limit _⟩
        exact outside (by simpa only [Bool.and_eq_true, decide_eq_true_eq] using range)

/-- Recover the latest actual marking boundary of each completed row from
a history containing arbitrary independent and nested resets. A retained old
tail and a fresh body may have different origins. Only this row is protected
in the suffix; independent bodies remain free to restart. -/
theorem a_completed_mixed_history_row_has_its_actual_commit_origin
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (history : ActorMeasuredHistory image choreo session first final cleared commits)
    (completed : final.1.doneState.done event = true) :
    (first.1.doneState.done event = true ∧ event ∉ cleared) ∨
    ∃ before after prior marked later priorCommits laterCommits record,
      ActorMeasuredHistory image choreo session first before prior priorCommits ∧
      ActorMeasuredCommit image choreo session before after marked record ∧
      ActorMeasuredHistory image choreo session after final later laterCommits ∧
      record.step.event = event ∧ event ∉ later := by
  induction history with
  | refl => exact .inl ⟨completed, by simp⟩
  | @tail middle final earlier more count added history step ih =>
    rcases a_measured_step_either_marks_the_row_or_preserves_its_origin step completed with
      ⟨record, marked, same⟩ | ⟨old, untouched⟩
    · exact .inr ⟨middle, final, earlier, more, [], count, 0, record,
        history, marked, .refl, same, by simp⟩
    · rcases ih old with ⟨initial, unreset⟩ |
        ⟨before, after, prior, marked, later, priorCommits, laterCommits, record,
          earlierHistory, publication, suffix, same, unreset⟩
      · exact .inl ⟨initial, by simpa only [List.mem_append, not_or] using And.intro unreset untouched⟩
      · exact .inr ⟨before, after, prior, marked, later ++ more,
          priorCommits, laterCommits + added, record, earlierHistory, publication,
          .tail suffix step, same, by simpa only [List.mem_append, not_or] using And.intro unreset untouched⟩

/-- A prerequisite in either source-derived candidate base is backed by a
real, source-and-wire-checked commit in the same actor history. Its completion
has not been reset since that commit. This does not assume value availability,
one common distributed iteration, or a successful candidate operation. -/
theorem a_ready_candidate_predecessor_has_its_mixed_history_origin
    {image : RustDescriptorImage} {choreo : Choreo} {session previousPath event : Nat}
    {sample : ActorAdmissionObservation} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow} {base first : ActorProgressObservation}
    {firstCount finalCount commits : Nat} {cleared : List Nat}
    (history : ActorMeasuredHistory image choreo session
      (first, firstCount) (sample.snapshot, finalCount) cleared commits)
    (initial : first.doneState.done event = false)
    (origin : ActorSourceCandidateBase sample image choreo previousPath path row base)
    (predecessor : event ∈ data.sourcePredecessors path row.eventId)
    (ready : sourceRowReady data path base row = true) :
    ∃ before after prior marked later priorCommits laterCommits record,
      ActorMeasuredHistory image choreo session (first, firstCount) before prior priorCommits ∧
      ActorMeasuredCommit image choreo session before after marked record ∧
      ActorMeasuredHistory image choreo session after (sample.snapshot, finalCount) later laterCommits ∧
      record.step.event = event ∧ event ∉ later ∧
      record.check image choreo session = true ∧ record.step.SourceOrdered image choreo := by
  have required := List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 event predecessor
  have completed : sample.snapshot.doneState.done event = true := by
    cases origin with
    | current agrees => exact required
    | fresh domain member prepared =>
      exact (a_prepared_source_base_only_retains_existing_completion prepared required).1
  rcases a_completed_mixed_history_row_has_its_actual_commit_origin history completed with
    ⟨old, _⟩ | ⟨before, after, prior, marked, later, priorCommits, laterCommits, record,
      earlierHistory, publication, suffix, same, unreset⟩
  · rw [initial] at old
    contradiction
  · exact ⟨before, after, prior, marked, later, priorCommits, laterCommits, record,
      earlierHistory, publication, suffix, same, unreset,
      a_measured_commit_retains_its_checked_source_and_wire publication⟩

end Hibana
