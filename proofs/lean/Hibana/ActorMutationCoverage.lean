import Hibana.ActorCommitHistory
import Hibana.ActorReentryCommit

namespace Hibana

/-- Measurements around an actual reentry poll. The reset range and prepared
base are derived by the existing source checker. Counts are qualification
observations, not a visit field or a runtime scheduling rule. -/
def actorReentryMutationCheck
    (sample : ActorReentryCommitObservation) (wire : OwnedWireObservation)
    (image : RustDescriptorImage) (choreo : Choreo) (session before after : Nat) : Bool :=
  match sample.prepared? image choreo with
  | none => false
  | some (roll, step) =>
      ({ step, wire } : ActorCommitRecord).check image choreo session &&
        after == before + roll.eventLength + 1

/-- A measured fresh visit uses the same checked source commit and accounts
for one clear of each projected body row and its one successful mark. No
global drain or reset of an independent sibling is part of this boundary. -/
theorem a_measured_reentry_has_its_source_reset_and_exact_writes
    {sample : ActorReentryCommitObservation} {wire : OwnedWireObservation}
    {image : RustDescriptorImage} {choreo : Choreo} {session before after : Nat}
    (checked : actorReentryMutationCheck sample wire image choreo session before after = true) :
    sample.SourceOrdered image choreo ∧
      ∃ roll step, sample.prepared? image choreo = some (roll, step) ∧
        ({ step, wire } : ActorCommitRecord).check image choreo session = true ∧
        after = before + roll.eventLength + 1 := by
  cases prepared : sample.prepared? image choreo with
  | none => simp [actorReentryMutationCheck, prepared] at checked
  | some entry =>
      rcases entry with ⟨roll, step⟩
      have fields : ({ step, wire } : ActorCommitRecord).check image choreo session = true ∧
          after = before + roll.eventLength + 1 := by
        simpa only [actorReentryMutationCheck, prepared, Bool.and_eq_true, beq_iff_eq] using checked
      have ordinary : step.check image choreo = true := checked_record_is_a_checked_commit fields.1
      have accepted : sample.check image choreo = true := by
        simpa only [ActorReentryCommitObservation.check, prepared] using ordinary
      exact ⟨accepted_body_reentry_restores_source_progress accepted,
        roll, step, rfl, fields⟩

/-- Retain actual progress outside the one source-validated reset range.
This applies to completed siblings and to still-pending independent work. -/
theorem a_measured_reentry_preserves_every_outside_progress_bit
    {sample : ActorReentryCommitObservation} {wire : OwnedWireObservation}
    {image : RustDescriptorImage} {choreo : Choreo} {session before after event : Nat}
    (checked : actorReentryMutationCheck sample wire image choreo session before after = true) :
    ∃ roll step rows,
      sample.prepared? image choreo = some (roll, step) ∧
      choreo.canonicalActorRollRows? image.role = some rows ∧ roll ∈ rows ∧
      ((¬(roll.eventStart ≤ event ∧ event < roll.eventStart + roll.eventLength)) →
        sample.after.doneState.done event = sample.before.doneState.done event) := by
  obtain ⟨ordered, _⟩ := a_measured_reentry_has_its_source_reset_and_exact_writes checked
  obtain ⟨roll, step, rows, prepared, sourceAt, member, reset, stepOrdered⟩ := ordered
  refine ⟨roll, step, rows, prepared, sourceAt, member, ?_⟩
  intro outside
  obtain ⟨_, data, path, row, binding, _, _, _, _, _, _, _, _, _, _,
    unchanged, _⟩ := stepOrdered
  have actual := prepared_body_reentry_keeps_its_actual_operation prepared
  have different : event ≠ step.event := by
    intro same
    exact outside (same.symm ▸ actual.2.2)
  rw [← actual.2.1, unchanged event different, reset]
  have notRange : ¬(roll.eventStart ≤ event &&
      event < roll.eventStart + roll.eventLength) = true := by simpa using outside
  simp only [if_neg notRange]

/-- Proof-only measurements at the two actual boundaries of a source commit.
The counters come from the cursor's qualification build, not from a harness
increment performed in place of the commit. They never drive an operation. -/
structure ActorCommitMutationRecord where
  commit : ActorCommitRecord
  before : Nat
  after : Nat

/-- Check a selected-visit history without an unrecorded completion-bit write
between its boundaries. Each recorded source commit performs one actual mark;
an intervening clear/reset must not be silently erased by equal snapshots.
Fresh reentry needs its own reset evidence and is not accepted as a zero-write
gap by this checker. A zero-write gap must preserve both observed progress
and committed choices exactly. Authentic observations remain an
implementation premise; a counter alone does not establish either equality. -/
def actorCommitMutationHistoryCheck
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (first : ActorProgressObservation) (firstMutation : Nat)
    (final : ActorProgressObservation) (finalMutation : Nat) :
    List ActorCommitMutationRecord → Bool
  | [] => (actorCommitGapCheck first final && decide (first.choices = final.choices)) &&
      (first.progress == final.progress && firstMutation == finalMutation)
  | record :: rest =>
      (actorCommitGapCheck first record.commit.step.before &&
        decide (first.choices = record.commit.step.before.choices)) &&
        (first.progress == record.commit.step.before.progress &&
          (firstMutation == record.before &&
            (record.after == record.before + 1 &&
              (record.commit.check image choreo session &&
                actorCommitMutationHistoryCheck image choreo session
                  record.commit.step.after record.after final finalMutation rest))))

theorem checked_mutation_history_has_its_checked_source_commits
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true) :
    actorCommitHistoryCheck image choreo session first final (records.map (·.commit)) = true := by
  induction records generalizing first firstMutation with
  | nil => exact (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).1
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      exact Bool.and_eq_true_iff.mpr ⟨Bool.and_eq_true_iff.mpr ⟨checked.1.1, checked.2.2.2.2.1⟩,
        ih checked.2.2.2.2.2⟩

/-- Quiet intervals cannot fabricate or forget branch knowledge. Genuine
fresh visits retain their separate source-derived reset/commit evidence. -/
theorem a_zero_write_measured_gap_preserves_committed_choices
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {before after : Nat}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first before final after [] = true) : final.choices = first.choices := by
  exact (of_decide_eq_true (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).2).symm

theorem checked_mutation_history_accounts_for_every_observed_write
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true) :
    finalMutation = firstMutation + records.length := by
  induction records generalizing first firstMutation with
  | nil =>
      exact (beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).2).symm
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      have firstAt := beq_iff_eq.mp checked.2.2.1
      have afterAt := beq_iff_eq.mp checked.2.2.2.1
      have finalAt := ih checked.2.2.2.2.2
      simp only [List.length_cons]
      omega

/-- Throughout a measured interval with no unrecorded reset, completed events
stay complete at the final cut and at every actual commit boundary. This does
not restrict roll: reentry is qualified using its separate reset evidence. -/
theorem a_measured_history_preserves_completed_events
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true)
    (completed : first.doneState.done event = true) :
    final.doneState.done event = true ∧
      ∀ record ∈ records, record.commit.step.before.doneState.done event = true ∧
        record.commit.step.after.doneState.done event = true := by
  induction records generalizing first firstMutation with
  | nil =>
      have progressAt := beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).1
      exact ⟨by simpa only [ActorProgressObservation.doneState, ← progressAt] using completed, by simp⟩
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      have progressAt := beq_iff_eq.mp checked.2.1
      have beforeDone : record.commit.step.before.doneState.done event = true := by
        simpa only [ActorProgressObservation.doneState, ← progressAt] using completed
      have afterDone := a_checked_source_commit_preserves_completed_events checked.2.2.2.2.1 beforeDone
      have tail := ih checked.2.2.2.2.2 afterDone
      refine ⟨tail.1, ?_⟩
      intro next member
      rcases List.mem_cons.mp member with same | member
      · subst next
        exact ⟨beforeDone, afterDone⟩
      · exact tail.2 next member

/-- If an event is still pending at the final cut, it was pending at every
measured commit. In particular an outgoing handoff cannot be replayed within
this interval by silently clearing its completion bit. -/
theorem a_pending_final_event_was_pending_at_every_measured_commit
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true)
    (pending : final.doneState.done event = false) :
    first.doneState.done event = false ∧
      ∀ record ∈ records, record.commit.step.before.doneState.done event = false ∧
        record.commit.step.after.doneState.done event = false := by
  induction records generalizing first firstMutation with
  | nil =>
      have progressAt := beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).1
      exact ⟨by simpa only [ActorProgressObservation.doneState, progressAt] using pending, by simp⟩
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      have tail := ih checked.2.2.2.2.2
      have beforePending : record.commit.step.before.doneState.done event = false := by
        cases doneAt : record.commit.step.before.doneState.done event with
        | false => rfl
        | true =>
            have afterDone := a_checked_source_commit_preserves_completed_events checked.2.2.2.2.1 doneAt
            rw [tail.1] at afterDone
            contradiction
      have progressAt := beq_iff_eq.mp checked.2.1
      refine ⟨by simpa only [ActorProgressObservation.doneState, progressAt] using beforePending, ?_⟩
      intro next member
      rcases List.mem_cons.mp member with same | member
      · subst next
        exact ⟨beforePending, tail.1⟩
      · exact tail.2 next member

/-- Unlike matching endpoint snapshots alone, actual differing mutation counts
reject an omitted reset/commit interval even if all ordinary fields agree. -/
theorem an_unrecorded_mutation_interval_is_rejected
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (first final : ActorProgressObservation) {firstMutation finalMutation : Nat}
    (different : firstMutation ≠ finalMutation) :
    actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation [] = false := by
  simp [actorCommitMutationHistoryCheck, different]

end Hibana
