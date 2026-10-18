import Hibana.ActorOccurrenceHistory
import Hibana.ActorChoicePublication

namespace Hibana

/-- A suffix of actual measured boundaries that does not reset this one
source route. Other route scopes, parallel bodies and actors remain free to
restart. This erased relation is not an execution ledger or scheduling rule. -/
inductive ActorChoiceUnresetHistory
    (image : RustDescriptorImage) (choreo : Choreo) (session scope : Nat) :
    ActorProgressObservation × Nat → ActorProgressObservation × Nat → Prop where
  | refl {state} : ActorChoiceUnresetHistory image choreo session scope state state
  | quiet {first : ActorProgressObservation × Nat} {before after : ActorProgressObservation} {count : Nat}
      (history : ActorChoiceUnresetHistory image choreo session scope first (before, count))
      (checked : actorCommitMutationHistoryCheck image choreo session before count after count [] = true) :
      ActorChoiceUnresetHistory image choreo session scope first (after, count)
  | commit {first : ActorProgressObservation × Nat} {record : ActorCommitMutationRecord}
      (history : ActorChoiceUnresetHistory image choreo session scope first (record.commit.step.before, record.before))
      (checked : record.commit.check image choreo session = true)
      (written : record.after = record.before + 1) :
      ActorChoiceUnresetHistory image choreo session scope first (record.commit.step.after, record.after)
  | fresh {first : ActorProgressObservation × Nat} {sample : ActorReentryCommitObservation}
      {wire : OwnedWireObservation} {before after : Nat} {roll : DecodedRollRow} {step : ActorCommitObservation}
      (history : ActorChoiceUnresetHistory image choreo session scope first (sample.before, before))
      (checked : actorReentryMutationCheck sample wire image choreo session before after = true)
      (prepared : sample.prepared? image choreo = some (roll, step))
      (outside : sourceRollResetsRoute? (canonicalProgramSource choreo) sample.scope scope = some false) :
      ActorChoiceUnresetHistory image choreo session scope first (sample.after, after)

private theorem unreset_choice_histories_compose
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat}
    {first middle final : ActorProgressObservation × Nat}
    (earlier : ActorChoiceUnresetHistory image choreo session scope first middle)
    (suffix : ActorChoiceUnresetHistory image choreo session scope middle final) :
    ActorChoiceUnresetHistory image choreo session scope first final := by
  induction suffix with
  | refl => exact earlier
  | quiet history checked ih => exact .quiet ih checked
  | commit history checked written ih => exact .commit ih checked written
  | fresh history checked prepared outside ih => exact .fresh ih checked prepared outside

private theorem a_checked_record_has_its_source_choice_publication
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat} {record : ActorCommitRecord}
    (checked : record.check image choreo session = true) :
    ∃ eff, image.eventEffIndex? record.step.event = some eff ∧
      publishedObservedChoices? (canonicalProgramSource choreo) eff
        record.step.before.choices = some record.step.after.choices := by
  obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
  obtain ⟨eff, atEff, published⟩ := Option.bind_eq_some_iff.mp publication
  exact ⟨eff, atEff, published⟩

/-- The operation begins without this local choice and publishes it from
its exact immutable source occurrence. A fresh operation's before state is
the source-derived prepared base, not the old pre-reset visible snapshot. -/
def ActorCommitRecord.IntroducesChoice
    (record : ActorCommitRecord) (image : RustDescriptorImage) (choreo : Choreo)
    (scope : Nat) (arm : RouteArm) : Prop :=
  record.step.before.choices.find? (fun row => row.1 == scope) = some (scope, none) ∧
  record.step.after.choices.find? (fun row => row.1 == scope) = some (scope, some arm) ∧
    ∃ eff memberships, image.eventEffIndex? record.step.event = some eff ∧
      canonicalEventRoutePath? (canonicalProgramSource choreo) eff = some memberships ∧
      memberships.find? (fun row => row.1 == scope) = some (scope, encodeRouteArm arm)

private theorem a_checked_record_preserves_or_introduces_the_selected_choice
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat} {arm : RouteArm}
    {record : ActorCommitRecord}
    (checked : record.check image choreo session = true)
    (known : record.step.after.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    record.step.before.choices.find? (fun row => row.1 == scope) = some (scope, some arm) ∨
      record.IntroducesChoice image choreo scope arm := by
  obtain ⟨eff, atEff, publication⟩ := a_checked_record_has_its_source_choice_publication checked
  rcases a_source_publication_has_an_existing_or_new_choice_origin publication known with old | fresh
  · exact .inl old
  · obtain ⟨unknown, memberships, pathAt, member⟩ := fresh
    exact .inr ⟨unknown, known, eff, memberships, atEff, pathAt, member⟩

theorem an_unreset_source_choice_survives_its_measured_suffix
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat} {arm : RouteArm}
    {first final : ActorProgressObservation × Nat}
    (history : ActorChoiceUnresetHistory image choreo session scope first final)
    (known : first.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    final.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  induction history with
  | refl => exact known
  | quiet history checked ih =>
    have same := a_zero_write_measured_gap_preserves_committed_choices checked
    simpa only [same] using ih
  | commit history checked written ih =>
    obtain ⟨_, _, publication⟩ := a_checked_record_has_its_source_choice_publication checked
    exact a_source_publication_preserves_existing_choice publication ih
  | fresh history checked prepared outside ih =>
    obtain ⟨base, resetAt, beforeAt, _, afterAt⟩ := prepared_body_reentry_is_derived_before_commit prepared
    have reset := a_prepared_body_has_its_source_choice_reset resetAt
    have baseKnows := a_source_choice_reset_preserves_outside_knowledge reset outside ih
    have commit := a_measured_commit_retains_its_checked_source_and_wire
      (ActorMeasuredCommit.fresh checked prepared)
    obtain ⟨_, _, publication⟩ := a_checked_record_has_its_source_choice_publication commit.1
    rw [beforeAt, afterAt] at publication
    exact a_source_publication_preserves_existing_choice publication baseKnows

private theorem a_measured_step_preserves_or_introduces_the_selected_choice
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat} {arm : RouteArm}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (step : ActorMeasuredStep image choreo session before after cleared commits)
    (known : after.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    (before.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm) ∧
      ActorChoiceUnresetHistory image choreo session scope before after) ∨
    ∃ record, ActorMeasuredCommit image choreo session before after cleared record ∧
      record.IntroducesChoice image choreo scope arm := by
  cases step with
  | quiet checked =>
    have same := a_zero_write_measured_gap_preserves_committed_choices checked
    exact .inl ⟨by simpa only [same] using known, .quiet .refl checked⟩
  | commit checked written =>
    rcases a_checked_record_preserves_or_introduces_the_selected_choice checked known with old | new
    · exact .inl ⟨old, .commit .refl checked written⟩
    · exact .inr ⟨_, .ordinary checked written, new⟩
  | reentry checked prepared =>
    have commit := a_measured_commit_retains_its_checked_source_and_wire
      (ActorMeasuredCommit.fresh checked prepared)
    obtain ⟨base, resetAt, beforeAt, _, afterAt⟩ := prepared_body_reentry_is_derived_before_commit prepared
    have actualKnown := afterAt.symm ▸ known
    rcases a_checked_record_preserves_or_introduces_the_selected_choice commit.1 actualKnown with old | new
    · rw [beforeAt] at old
      have reset := a_prepared_body_has_its_source_choice_reset resetAt
      obtain ⟨outside, previous⟩ := a_source_choice_reset_only_retains_unreset_knowledge reset old
      exact .inl ⟨previous, .fresh .refl checked prepared outside⟩
    · exact .inr ⟨_, .fresh checked prepared, new⟩

private theorem a_measured_history_choice_has_its_initial_or_latest_source_origin
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat} {arm : RouteArm}
    {first final : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (history : ActorMeasuredHistory image choreo session first final cleared commits)
    (known : final.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    (first.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm) ∧
      ActorChoiceUnresetHistory image choreo session scope first final) ∨
    ∃ before after prior marked priorCommits record,
      ActorMeasuredHistory image choreo session first before prior priorCommits ∧
      ActorMeasuredCommit image choreo session before after marked record ∧
      record.IntroducesChoice image choreo scope arm ∧
      ActorChoiceUnresetHistory image choreo session scope after final := by
  induction history with
  | refl => exact .inl ⟨known, .refl⟩
  | @tail middle final earlier more count added history step ih =>
    rcases a_measured_step_preserves_or_introduces_the_selected_choice step known with
      ⟨old, retained⟩ | ⟨record, committed, introduced⟩
    · rcases ih old with ⟨initial, whole⟩ |
        ⟨before, after, prior, marked, priorCommits, record, earlierHistory, committed, introduced, suffix⟩
      · exact .inl ⟨initial, unreset_choice_histories_compose whole retained⟩
      · exact .inr ⟨before, after, prior, marked, priorCommits, record, earlierHistory, committed,
          introduced, unreset_choice_histories_compose suffix retained⟩
    · exact .inr ⟨middle, final, earlier, more, count, record, history, committed, introduced, .refl⟩

/-- Recover the current local choice's actual introducing commit through
ordinary steps and arbitrary independent/nested restarts. Its suffix contains
no reset of this route. Thus an equal arm selected after a reset cannot reuse
an old visit's receipt. Other actors need not share a complete path or visit. -/
theorem a_current_choice_has_its_latest_actual_source_origin
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat} {arm : RouteArm}
    {first final : ActorProgressObservation × Nat} {cleared : List Nat} {commits : Nat}
    (history : ActorMeasuredHistory image choreo session first final cleared commits)
    (initial : first.1.choices.find? (fun row => row.1 == scope) = some (scope, none))
    (known : final.1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    ∃ before after prior marked priorCommits record,
      ActorMeasuredHistory image choreo session first before prior priorCommits ∧
      ActorMeasuredCommit image choreo session before after marked record ∧
      record.IntroducesChoice image choreo scope arm ∧
      ActorChoiceUnresetHistory image choreo session scope after final ∧
      record.check image choreo session = true ∧ record.step.SourceOrdered image choreo := by
  rcases a_measured_history_choice_has_its_initial_or_latest_source_origin history known with
    ⟨old, _⟩ | ⟨before, after, prior, marked, priorCommits, record, earlierHistory, committed, introduced, suffix⟩
  · rw [initial] at old
    cases old
  · exact ⟨before, after, prior, marked, priorCommits, record, earlierHistory, committed, introduced,
      suffix, a_measured_commit_retains_its_checked_source_and_wire committed⟩

end Hibana
