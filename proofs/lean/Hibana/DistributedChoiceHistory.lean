import Hibana.DistributedSourceExecution

namespace Hibana
open EndpointDelegation

private theorem a_quiet_actor_interval_preserves_its_source_choice
    {image : RustDescriptorImage} {choreo : Choreo} {session scope : Nat}
    {before after : ActorProgressObservation × Nat}
    (checked : actorCommitMutationHistoryCheck image choreo session
      before.1 before.2 after.1 after.2 [] = true) :
    ActorChoiceUnresetHistory image choreo session scope before after := by
  rcases before with ⟨before, count⟩
  rcases after with ⟨after, next⟩
  have same : next = count := by
    simpa only [List.length_nil, Nat.add_zero] using
      checked_mutation_history_accounts_for_every_observed_write checked
  subst next
  exact .quiet .refl checked

private theorem a_distributed_step_preserves_or_introduces_its_actors_choice
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor scope : Nat} {arm : RouteArm}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged)
    (known : (last.1.actors actor).1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    ((first.1.actors actor).1.choices.find? (fun row => row.1 == scope) = some (scope, some arm) ∧
      ActorChoiceUnresetHistory (images actor) choreo session scope (first.1.actors actor) (last.1.actors actor)) ∨
    ∃ record cleared, logged = some (actor, record) ∧
      ActorMeasuredCommit (images actor) choreo session
        (first.1.actors actor) (last.1.actors actor) cleared record ∧
      record.IntroducesChoice (images actor) choreo scope arm := by
  rcases a_distributed_step_preserves_or_measures_its_actor step actor with
    same | quiet | ⟨record, cleared, loggedAt, committed⟩
  · exact .inl ⟨by rw [same]; exact known, by rw [← same]; exact .refl⟩
  · have choices := a_zero_write_measured_gap_preserves_committed_choices quiet
    exact .inl ⟨by simpa only [choices] using known, a_quiet_actor_interval_preserves_its_source_choice quiet⟩
  · rcases a_measured_commit_preserves_or_introduces_its_selected_choice committed known with old | introduced
    · exact .inl old
    · exact .inr ⟨record, cleared, loggedAt, committed, introduced⟩

/-- A current local choice has its introducing boundary in this same global
history. Its retained suffix contains no reset of this source route. Other
actors, independent bodies, nested routes and unknown sibling slots are free
to differ. Equal arm values before/after a fresh visit do not reuse its old
receipt, because introduction is checked at the actual prepared base. -/
theorem a_current_distributed_choice_has_its_unreset_actual_boundary
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor scope : Nat} {arm : RouteArm}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : (first.1.actors actor).1.choices.find? (fun row => row.1 == scope) = some (scope, none))
    (known : (last.1.actors actor).1.choices.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    ∃ before after prior later record cleared,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after (some (actor, record)) ∧
      ActorMeasuredCommit (images actor) choreo session
        (before.1.actors actor) (after.1.actors actor) cleared record ∧
      record.IntroducesChoice (images actor) choreo scope arm ∧
      DistributedSourceExecution images choreo session after last later ∧
      ActorChoiceUnresetHistory (images actor) choreo session scope
        (after.1.actors actor) (last.1.actors actor) := by
  induction execution with
  | refl => rw [initial] at known; cases known
  | @tail middle last prior logged history step ih =>
    rcases a_distributed_step_preserves_or_introduces_its_actors_choice step known with
      ⟨old, retained⟩ | ⟨record, cleared, loggedAt, committed, introduced⟩
    · obtain ⟨before, after, earlier, later, record, cleared, prefixHistory, boundary,
        actual, origin, suffixHistory, unreset⟩ := ih old
      exact ⟨before, after, earlier, _, record, cleared, prefixHistory, boundary,
        actual, origin, .tail suffixHistory step, unreset_source_choice_histories_compose unreset retained⟩
    · exact ⟨middle, last, prior, [], record, cleared, history, loggedAt ▸ step,
        committed, introduced, .refl, .refl⟩

end Hibana
