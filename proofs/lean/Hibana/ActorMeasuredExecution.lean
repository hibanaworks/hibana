import Hibana.ActorMutationCoverage

namespace Hibana

/-- Proof-only boundaries of actual actor progress. A fresh boundary names
the same source-derived prepared operation as its real poll; its cleared rows
are not supplied by a caller or stored in the runtime. -/
inductive ActorMeasuredStep (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    ActorProgressObservation × Nat → ActorProgressObservation × Nat → List Nat → Nat → Prop where
  | quiet {before after : ActorProgressObservation} {count : Nat}
      (checked : actorCommitMutationHistoryCheck image choreo session before count after count [] = true) :
      ActorMeasuredStep image choreo session (before, count) (after, count) [] 0
  | commit {record : ActorCommitMutationRecord}
      (checked : record.commit.check image choreo session = true)
      (written : record.after = record.before + 1) :
      ActorMeasuredStep image choreo session
        (record.commit.step.before, record.before) (record.commit.step.after, record.after) [] 1
  | reentry {sample : ActorReentryCommitObservation} {wire : OwnedWireObservation}
      {before after : Nat} {roll : DecodedRollRow} {step : ActorCommitObservation}
      (checked : actorReentryMutationCheck sample wire image choreo session before after = true)
      (prepared : sample.prepared? image choreo = some (roll, step)) :
      ActorMeasuredStep image choreo session (sample.before, before) (sample.after, after)
        ((List.range roll.eventLength).map (roll.eventStart + ·))
        (rollCompletionWriteCount image roll + 1)

/-- A finite measured history can mix ordinary commits and independently
restarted bodies. The cleared event rows and remaining write count are erased observations used
to check the real write count, not a second execution or ownership authority. -/
inductive ActorMeasuredHistory (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    ActorProgressObservation × Nat → ActorProgressObservation × Nat → List Nat → Nat → Prop where
  | refl {state} : ActorMeasuredHistory image choreo session state state [] 0
  | tail {first middle final : ActorProgressObservation × Nat} {cleared more : List Nat}
      {commits added : Nat}
      (history : ActorMeasuredHistory image choreo session first middle cleared commits)
      (step : ActorMeasuredStep image choreo session middle final more added) :
      ActorMeasuredHistory image choreo session first final (cleared ++ more) (commits + added)

/-- Join actual measured intervals without adding a barrier or excluding
fresh visits. Only their common observed boundary must be identical. -/
theorem measured_actor_histories_compose_at_the_same_boundary
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first middle final : ActorProgressObservation × Nat} {cleared more : List Nat}
    {commits added : Nat}
    (earlierHistory : ActorMeasuredHistory image choreo session first middle cleared commits)
    (suffix : ActorMeasuredHistory image choreo session middle final more added) :
    ActorMeasuredHistory image choreo session first final (cleared ++ more) (commits + added) := by
  induction suffix with
  | refl => simpa only [List.append_nil, Nat.add_zero] using earlierHistory
  | tail history step ih =>
      simpa only [List.append_assoc, Nat.add_assoc] using ActorMeasuredHistory.tail ih step

private theorem a_measured_step_accounts_for_its_marks_and_clears
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (step : ActorMeasuredStep image choreo session before after cleared commits) :
    after.2 = before.2 + cleared.length + commits := by
  cases step with
  | quiet checked => simp
  | commit checked written => simpa only [List.length_nil, Nat.add_zero] using written
  | @reentry sample wire first final roll operation checked prepared =>
      obtain ⟨_, actualRoll, actualStep, actualAt, _, written⟩ :=
        a_measured_reentry_has_its_source_reset_and_exact_writes checked
      have same := Option.some.inj (actualAt.symm.trans prepared)
      have rollAt : actualRoll = roll := congrArg Prod.fst same
      subst actualRoll
      simpa only [List.length_map, List.length_range, Nat.add_assoc] using written

private theorem a_measured_step_retains_untouched_completion
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (step : ActorMeasuredStep image choreo session before after cleared commits)
    (untouched : event ∉ cleared) (completed : before.1.doneState.done event = true) :
    after.1.doneState.done event = true := by
  cases step with
  | quiet checked =>
      have progressAt := beq_iff_eq.mp
        (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).1
      simpa only [ActorProgressObservation.doneState, ← progressAt] using completed
  | commit checked written =>
      exact a_checked_source_commit_preserves_completed_events checked completed
  | @reentry sample wire first final roll operation checked prepared =>
      obtain ⟨actualRoll, actualStep, rows, actualAt, _, _, preserved⟩ :=
        a_measured_reentry_preserves_every_outside_progress_bit (event := event) checked
      have same := Option.some.inj (actualAt.symm.trans prepared)
      have rollAt : actualRoll = roll := congrArg Prod.fst same
      subst actualRoll
      have outside : ¬(roll.eventStart ≤ event ∧ event < roll.eventStart + roll.eventLength) := by
        intro inside
        have offset : event - roll.eventStart < roll.eventLength := by omega
        exact untouched (List.mem_map.mpr
          ⟨event - roll.eventStart, List.mem_range.mpr offset, by omega⟩)
      exact (preserved outside).trans completed

/-- Every observed write in a mixed fresh/ordinary history is an actual
projected body clear or a checked successful mark. No visit is treated as an
invisible gap merely because its endpoint snapshots happen to coincide. -/
theorem a_measured_actor_history_accounts_for_every_mark_and_clear
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (history : ActorMeasuredHistory image choreo session before after cleared commits) :
    after.2 = before.2 + cleared.length + commits := by
  induction history with
  | refl => simp
  | tail history step ih =>
      have last := a_measured_step_accounts_for_its_marks_and_clears step
      simp only [List.length_append]
      omega

/-- An independent roll can clear any number of its own rows. Completion of
a required row survives the entire history if no actual reset contains it.
This is a row-local condition, not a ban on resets anywhere in the actor. -/
theorem a_measured_actor_history_preserves_completion_outside_its_resets
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (history : ActorMeasuredHistory image choreo session before after cleared commits)
    (untouched : event ∉ cleared) (completed : before.1.doneState.done event = true) :
    after.1.doneState.done event = true := by
  induction history with
  | refl => exact completed
  | tail history step ih =>
      have previous : event ∉ _ := fun member => untouched (List.mem_append.mpr (.inl member))
      have current : event ∉ _ := fun member => untouched (List.mem_append.mpr (.inr member))
      exact a_measured_step_retains_untouched_completion step current (ih previous)

end Hibana
