import Hibana.ActorCommitHistory
import Hibana.ActorReentryCommit

namespace Hibana

/-- A sealed role image allocates borrow epochs when a repeated source region
contains a complete parallel operation with at least one local event. Compare
global effect offsets here, since projection can make separate intervals coincide. -/
def RustDescriptorImage.usesBorrowEpochs (image : RustDescriptorImage) : Bool := Id.run do
  let some rows := image.decodeRollRows? | return false
  let some markers := image.decodeScopeMarkers? | return false
  let some localEvents := image.decodeEventEffIndices? | return false
  let entries := markers.zipIdx.filter (fun (marker, _) => marker.tag % 4 == 0)
  return entries.any fun (par, index) =>
    par.scope / productionScopeCapacity == 2 && Id.run do
      let stop := scopeSegmentEnd markers index image.atomCount par
      return localEvents.any (fun eff => par.offset ≤ eff && eff < stop) &&
        entries.any fun (marker, i) =>
          marker.scope / productionScopeCapacity == 1 &&
            marker.offset ≤ par.offset && stop ≤ scopeSegmentEnd markers i image.atomCount marker &&
            rows.any (fun row => row.scope == marker.scope % productionScopeCapacity)

private def scopeResetWrites (hasEpochs : Bool) (body : List DecodedRollRow)
    (preserved : List DecodedRollRow) : Nat :=
  (body.map fun row =>
    if preserved.any (fun outer => row.scope != outer.scope && row.nestedWithin outer) then 0
    else if preserved.contains row then 1
    else if hasEpochs then 3 else 1).sum

/-- Each contained scope writes its completion bit. With borrow epochs it
also writes its epoch and credit, including a replacement by the same value. -/
def rollCompletionWriteCount (image : RustDescriptorImage) (roll : DecodedRollRow) : Nat :=
  let body := (image.decodeRollRows?.getD []).filter (·.nestedWithin roll)
  scopeResetWrites image.usesBorrowEpochs body []

theorem a_whole_body_reset_charges_each_contained_scope
    (image : RustDescriptorImage) (roll : DecodedRollRow) :
    rollCompletionWriteCount image roll =
      ((image.decodeRollRows?.getD []).filter (·.nestedWithin roll)).length *
        (if image.usesBorrowEpochs then 3 else 1) := by
  unfold rollCompletionWriteCount scopeResetWrites
  induction ((image.decodeRollRows?.getD []).filter (·.nestedWithin roll)) with
  | nil => simp
  | cons head rest ih => simp_all [Nat.succ_mul, Nat.add_comm]

/-- Count a single reset given the source-independent credited child rows.
An outer preserved child spends one credit. All scopes and events beneath
that child keep their state, even if an inner child also has a credit.
A successful operation subsequently marks its own event once. -/
def rollResetWriteCount (image : RustDescriptorImage) (roll : DecodedRollRow)
    (preserved : List DecodedRollRow) : Nat :=
  let hasEpochs := image.usesBorrowEpochs
  let body := (image.decodeRollRows?.getD []).filter (·.nestedWithin roll)
  let kept := if !hasEpochs || preserved.isEmpty then [] else
    body.filter (fun child => child.scope != roll.scope && preserved.contains child)
  let cleared := (List.range roll.eventLength).filter fun i =>
    !kept.any (fun child => child.eventStart ≤ roll.eventStart + i &&
      roll.eventStart + i < child.eventStart + child.eventLength)
  cleared.length + scopeResetWrites hasEpochs body kept

theorem a_whole_body_reset_counts_every_event_and_scope_write
    (image : RustDescriptorImage) (roll : DecodedRollRow) :
    rollResetWriteCount image roll [] = roll.eventLength + rollCompletionWriteCount image roll := by
  have all : (List.range roll.eventLength).filter (fun _ => true) = List.range roll.eventLength :=
    List.filter_eq_self.mpr (by simp)
  simp [rollResetWriteCount, rollCompletionWriteCount, all]

/-- Measurements for a single whole-body reset with no preserved child.
The physical constructor currently prepares exactly that reset. Its endpoint
poll also contributes the final event mark. -/
def actorReentryMutationCheck
    (sample : ActorReentryCommitObservation) (wire : OwnedWireObservation)
    (image : RustDescriptorImage) (choreo : Choreo) (session before after : Nat) : Bool :=
  match sample.prepared? image choreo with
  | none => false
  | some (roll, step) =>
      ({ step, wire } : ActorCommitRecord).check image choreo session &&
        after == before + roll.eventLength + rollCompletionWriteCount image roll + 1

/-- A measured fresh visit uses the same checked source commit and accounts
for each body-row clear, scope metadata write and successful event mark. -/
theorem a_measured_reentry_has_its_source_reset_and_exact_writes
    {sample : ActorReentryCommitObservation} {wire : OwnedWireObservation}
    {image : RustDescriptorImage} {choreo : Choreo} {session before after : Nat}
    (checked : actorReentryMutationCheck sample wire image choreo session before after = true) :
    sample.SourceOrdered image choreo ∧
      ∃ roll step, sample.prepared? image choreo = some (roll, step) ∧
        ({ step, wire } : ActorCommitRecord).check image choreo session = true ∧
        after = before + roll.eventLength + rollCompletionWriteCount image roll + 1 := by
  cases prepared : sample.prepared? image choreo with
  | none => simp [actorReentryMutationCheck, prepared] at checked
  | some entry =>
      rcases entry with ⟨roll, step⟩
      have fields : ({ step, wire } : ActorCommitRecord).check image choreo session = true ∧
          after = before + roll.eventLength + rollCompletionWriteCount image roll + 1 := by
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
