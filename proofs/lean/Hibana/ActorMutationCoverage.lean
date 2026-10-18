import Hibana.ActorCommitHistory

namespace Hibana

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
gap by this checker. Authentic observations remain an implementation premise. -/
def actorCommitMutationHistoryCheck
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (first : ActorProgressObservation) (firstMutation : Nat)
    (final : ActorProgressObservation) (finalMutation : Nat) :
    List ActorCommitMutationRecord → Bool
  | [] => actorCommitGapCheck first final && firstMutation == finalMutation
  | record :: rest =>
      actorCommitGapCheck first record.commit.step.before &&
        (firstMutation == record.before &&
          (record.after == record.before + 1 &&
            (record.commit.check image choreo session &&
              actorCommitMutationHistoryCheck image choreo session
                record.commit.step.after record.after final finalMutation rest)))

theorem checked_mutation_history_has_its_checked_source_commits
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true) :
    actorCommitHistoryCheck image choreo session first final (records.map (·.commit)) = true := by
  induction records generalizing first firstMutation with
  | nil => exact (Bool.and_eq_true_iff.mp checked).1
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      exact Bool.and_eq_true_iff.mpr ⟨Bool.and_eq_true_iff.mpr ⟨checked.1, checked.2.2.2.1⟩,
        ih checked.2.2.2.2⟩

theorem checked_mutation_history_accounts_for_every_observed_write
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {firstMutation finalMutation : Nat}
    {records : List ActorCommitMutationRecord}
    (checked : actorCommitMutationHistoryCheck image choreo session
      first firstMutation final finalMutation records = true) :
    finalMutation = firstMutation + records.length := by
  induction records generalizing first firstMutation with
  | nil =>
      exact (beq_iff_eq.mp (Bool.and_eq_true_iff.mp checked).2).symm
  | cons record rest ih =>
      simp only [actorCommitMutationHistoryCheck, Bool.and_eq_true] at checked
      have firstAt := beq_iff_eq.mp checked.2.1
      have afterAt := beq_iff_eq.mp checked.2.2.1
      have finalAt := ih checked.2.2.2.2
      simp only [List.length_cons]
      omega

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
