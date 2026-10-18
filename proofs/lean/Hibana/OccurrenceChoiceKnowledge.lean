import Hibana.SendAdmission

namespace Hibana

private theorem a_published_choice_keeps_its_local_scope
    {memberships : List (Nat × Nat)} {entry result : Nat × Option RouteArm}
    (published : publishObservedChoice? memberships entry = some result) :
    result.1 = entry.1 := by
  rcases entry with ⟨scope, previous⟩
  unfold publishObservedChoice? at published
  cases atScope : memberships.find? (fun member => member.1 == scope) with
  | none =>
    simp only [atScope, Option.pure_def, Option.some.injEq] at published
    exact (congrArg Prod.fst published).symm
  | some member =>
    simp only [atScope] at published
    split at published <;>
      simp only [Option.bind_eq_bind, Option.bind_some, Option.bind_none, Option.pure_def] at published
    all_goals first | contradiction | (split at published <;> simp_all <;> cases published <;> rfl)

theorem a_published_occurrence_teaches_its_local_route_arm
    {memberships : List (Nat × Nat)} {scope : Nat} {previous : Option RouteArm}
    {arm : RouteArm} {result : Nat × Option RouteArm}
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (published : publishObservedChoice? memberships (scope, previous) = some result) :
    result = (scope, some arm) := by
  cases arm <;>
    simp only [publishObservedChoice?, membership, encodeRouteArm, Option.bind_eq_bind, Option.bind_some] at published <;>
    split at published <;> simp_all

private theorem successful_mapM_preserves_first_key
    {f : (Nat × Option RouteArm) → Option (Nat × Option RouteArm)}
    {before after : List (Nat × Option RouteArm)} {scope : Nat} {entry : Nat × Option RouteArm}
    (keepsKey : ∀ input output, f input = some output → output.1 = input.1)
    (mapped : before.mapM f = some after)
    (found : before.find? (fun row => row.1 == scope) = some entry) :
    ∃ result, f entry = some result ∧
      after.find? (fun row => row.1 == scope) = some result := by
  induction before generalizing after with
  | nil => simp at found
  | cons head tail ih =>
    cases headAt : f head with
    | none => simp [headAt] at mapped
    | some output =>
      cases tailAt : tail.mapM f with
      | none => simp [headAt, tailAt] at mapped
      | some rest =>
        have same : output :: rest = after := Option.some.inj (by simpa [headAt, tailAt] using mapped)
        subst after
        have key := keepsKey head output headAt
        cases sameScope : head.1 == scope with
        | true =>
          have entrySame : head = entry := by
            simpa only [List.find?_cons, sameScope, if_true, Option.some.injEq] using found
          subst entry
          exact ⟨output, headAt, by simp only [List.find?_cons, key, sameScope]⟩
        | false =>
          have remains : tail.find? (fun row => row.1 == scope) = some entry := by
            simpa only [List.find?_cons, sameScope, Bool.false_eq_true, if_false] using found
          obtain ⟨result, atEntry, inRest⟩ := ih tailAt remains
          exact ⟨result, atEntry, by simp only [List.find?_cons, key, sameScope, inRest]⟩

/-- The same source occurrence publishes only the branch knowledge represented
by this actor's local slots. Actors need not share all choices or one complete
source path, and no new decision is inferred from application payload bytes. -/
theorem a_source_occurrence_publishes_its_observed_local_choice
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : RouteArm}
    {memberships : List (Nat × Nat)} {before after : List (Nat × Option RouteArm)}
    {previous : Option RouteArm}
    (pathAt : canonicalEventRoutePath? source eff = some memberships)
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (localSlot : before.find? (fun row => row.1 == scope) = some (scope, previous))
    (published : publishedObservedChoices? source eff before = some after) :
    after.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  have mapped : before.mapM (publishObservedChoice? memberships) = some after := by
    simpa only [publishedObservedChoices?, pathAt, Option.bind_some] using published
  obtain ⟨result, atEntry, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped localSlot
  rw [a_published_occurrence_teaches_its_local_route_arm membership atEntry] at found
  exact found

theorem a_source_occurrence_preserves_an_unrelated_local_choice
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : Option RouteArm}
    {memberships : List (Nat × Nat)} {before after : List (Nat × Option RouteArm)}
    (pathAt : canonicalEventRoutePath? source eff = some memberships)
    (unrelated : memberships.find? (fun member => member.1 == scope) = none)
    (localSlot : before.find? (fun row => row.1 == scope) = some (scope, arm))
    (published : publishedObservedChoices? source eff before = some after) :
    after.find? (fun row => row.1 == scope) = some (scope, arm) := by
  have mapped : before.mapM (publishObservedChoice? memberships) = some after := by
    simpa only [publishedObservedChoices?, pathAt, Option.bind_some] using published
  obtain ⟨result, atEntry, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped localSlot
  have same : result = (scope, arm) := by
    simpa only [publishObservedChoice?, unrelated, Option.pure_def, Option.some.injEq] using atEntry.symm
  simpa only [same] using found

private theorem a_checked_owned_boundary_has_its_choice_publication
    {receiver : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {session : Nat} {wire : OwnedWireObservation} {facts : OwnedCommitFacts}
    (checked : receiver.ownedCheck image choreo wire session = true)
    (factsAt : receiver.ownedFacts? image choreo = some facts) :
    publishedObservedChoices? (canonicalProgramSource choreo) facts.effIndex
      receiver.before.choices = some receiver.after.choices := by
  have columns := decoded_owned_source_facts_bind_wire_columns
    (decoded_owned_commit_retains_its_source_facts factsAt)
  obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress (Bool.and_eq_true_iff.mp checked).1
  simpa only [columns.1, Option.bind_some, OwnedCommitFacts.sourceFacts] using publication

/-- The exact accepted owned frame teaches the receiver its own branch slot.
The sender contributes immutable pre-publication source facts, not a successful
sender commit. The two actors may have unrelated choices or rolled visits. -/
theorem a_checked_owned_receipt_learns_its_accepted_source_choice
    {sender : SourceSendAdmission} {receiver : ActorCommitObservation}
    {receiverImage : RustDescriptorImage} {choreo : Choreo} {session scope : Nat}
    {sentFacts : OwnedSourceFacts} {receivedFacts : OwnedCommitFacts} {wire : OwnedWireObservation}
    {memberships : List (Nat × Nat)} {arm : RouteArm} {previous : Option RouteArm}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck receiverImage choreo wire session = true)
    (receivedAt : receiver.ownedFacts? receiverImage choreo = some receivedFacts)
    (receiving : receivedFacts.side = .receive)
    (preserved : sender.wire.payload = wire.payload)
    (pathAt : canonicalEventRoutePath? (canonicalProgramSource choreo) sentFacts.effIndex = some memberships)
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (localSlot : receiver.before.choices.find? (fun row => row.1 == scope) = some (scope, previous)) :
    receiver.after.actor = receivedFacts.binding.receiverActor ∧
      receiver.after.doneState.selected scope = some arm := by
  refine ⟨checked_owned_receive_publishes_its_projected_actor checked receivedAt receiving, ?_⟩
  have sameEvent := (accepted_owned_source_bytes_identify_the_checked_receiver
    accepted sentAt checked receivedAt preserved).2
  have publication := a_checked_owned_boundary_has_its_choice_publication checked receivedAt
  rw [← sameEvent] at publication
  have learned := a_source_occurrence_publishes_its_observed_local_choice
    pathAt membership localSlot publication
  simp only [ActorProgressObservation.doneState, learned, Option.bind_some]

/-- Sharing one accepted frame does not share a whole route history. A local
slot outside that occurrence's memberships retains its previous knowledge,
including `none` for a branch the participant has not entered. -/
theorem a_checked_owned_frame_preserves_an_unrelated_local_choice
    {sender : SourceSendAdmission} {receiver : ActorCommitObservation}
    {receiverImage : RustDescriptorImage} {choreo : Choreo} {session scope : Nat}
    {sentFacts : OwnedSourceFacts} {receivedFacts : OwnedCommitFacts} {wire : OwnedWireObservation}
    {memberships : List (Nat × Nat)} {previous : Option RouteArm}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck receiverImage choreo wire session = true)
    (receivedAt : receiver.ownedFacts? receiverImage choreo = some receivedFacts)
    (preserved : sender.wire.payload = wire.payload)
    (pathAt : canonicalEventRoutePath? (canonicalProgramSource choreo) sentFacts.effIndex = some memberships)
    (unrelated : memberships.find? (fun member => member.1 == scope) = none)
    (localSlot : receiver.before.choices.find? (fun row => row.1 == scope) = some (scope, previous)) :
    receiver.after.doneState.selected scope = previous := by
  have sameEvent := (accepted_owned_source_bytes_identify_the_checked_receiver
    accepted sentAt checked receivedAt preserved).2
  have publication := a_checked_owned_boundary_has_its_choice_publication checked receivedAt
  rw [← sameEvent] at publication
  have kept := a_source_occurrence_preserves_an_unrelated_local_choice
    pathAt unrelated localSlot publication
  simp only [ActorProgressObservation.doneState, kept, Option.bind_some]

end Hibana
