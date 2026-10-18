import Hibana.SendAdmission
import Hibana.ActorChoicePublication
import Hibana.ActorChoiceOrigin

namespace Hibana

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

/-- The same accepted frame remains the basis of this receiver's current
choice through its actual unreset suffix. This proves branch correspondence,
not possession of the delegated value at the later cut. A return may already
have moved that value away. Sender publication is still not a premise. -/
theorem a_received_choice_stays_tied_to_its_accepted_source_occurrence
    {sender : SourceSendAdmission} {receiver : ActorCommitObservation}
    {receiverImage : RustDescriptorImage} {choreo : Choreo} {session scope : Nat}
    {sentFacts : OwnedSourceFacts} {receivedFacts : OwnedCommitFacts} {wire : OwnedWireObservation}
    {memberships : List (Nat × Nat)} {arm : RouteArm} {previous : Option RouteArm}
    {final : ActorProgressObservation × Nat} {count : Nat}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck receiverImage choreo wire session = true)
    (receivedAt : receiver.ownedFacts? receiverImage choreo = some receivedFacts)
    (receiving : receivedFacts.side = .receive)
    (preserved : sender.wire.payload = wire.payload)
    (pathAt : canonicalEventRoutePath? (canonicalProgramSource choreo) sentFacts.effIndex = some memberships)
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (localSlot : receiver.before.choices.find? (fun row => row.1 == scope) = some (scope, previous))
    (suffix : ActorChoiceUnresetHistory receiverImage choreo session scope
      (receiver.after, count) final) :
    receiver.after.actor = receivedFacts.binding.receiverActor ∧
      final.1.doneState.selected scope = some arm := by
  refine ⟨checked_owned_receive_publishes_its_projected_actor checked receivedAt receiving, ?_⟩
  have sameEvent := (accepted_owned_source_bytes_identify_the_checked_receiver
    accepted sentAt checked receivedAt preserved).2
  have publication := a_checked_owned_boundary_has_its_choice_publication checked receivedAt
  rw [← sameEvent] at publication
  have learned := a_source_occurrence_publishes_its_observed_local_choice
    pathAt membership localSlot publication
  have current := an_unreset_source_choice_survives_its_measured_suffix suffix learned
  simp only [ActorProgressObservation.doneState, current, Option.bind_some]

end Hibana
