import Hibana.ActorAdmission

namespace Hibana
open EndpointDelegation

/-- Immutable source facts available before transport acceptance. There is no
post-send snapshot, receiver tag or sender-publication premise in this data. -/
structure OwnedSourceFacts where
  effIndex : Nat
  resource : EndpointKey
  binding : Binding
  side : Side
  lane : Nat
  frameLabel : Nat

def RustDescriptorImage.ownedSourceFacts? (image : RustDescriptorImage)
    (choreo : Choreo) (pathIndex event : Nat) : Option OwnedSourceFacts := do
  let data ← image.decodeActorOrder? choreo
  let path ← data.normalized.paths[pathIndex]?
  let row ← data.rows[event]?
  let (_, binding) ← path.bindings.find? fun entry => entry.1 == row.actionId.event
  let .endpoint resource := binding.message.payload | none
  let effIndex ← image.eventEffIndex? event
  let lane ← image.eventLane? event
  let frameLabel ← image.eventFrameLabel? event
  pure { effIndex, resource, binding, side := row.actionId.side, lane, frameLabel }

def OwnedSourceFacts.actor? (facts : OwnedSourceFacts) : Option Nat :=
  match facts.side with
  | .send => some facts.binding.senderActor
  | .receive => some facts.binding.receiverActor
  | .local => none

def OwnedSourceFacts.envelope (facts : OwnedSourceFacts) : List Nat :=
  [facts.resource.role, facts.effIndex / 256, facts.effIndex % 256]

theorem decoded_owned_source_facts_retain_the_exact_source_row
    {image : RustDescriptorImage} {choreo : Choreo} {pathIndex event : Nat}
    {facts : OwnedSourceFacts}
    (decoded : image.ownedSourceFacts? choreo pathIndex event = some facts) :
    ∃ data path row entry,
      image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[pathIndex]? = some path ∧ data.rows[event]? = some row ∧
      path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
      entry.2.message.payload = .endpoint facts.resource ∧ row.actionId.side = facts.side ∧
      facts.binding = entry.2 := by
  change (image.decodeActorOrder? choreo).bind _ = some facts at decoded
  obtain ⟨data, dataAt, remaining⟩ := Option.bind_eq_some_iff.mp decoded
  obtain ⟨path, pathAt, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  obtain ⟨row, rowAt, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  obtain ⟨entry, entryAt, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  dsimp only at remaining
  cases payloadAt : entry.2.message.payload with
  | wire schema => simp only [payloadAt] at remaining; contradiction
  | endpoint resource =>
    simp only [payloadAt] at remaining
    obtain ⟨eff, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
    obtain ⟨lane, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
    obtain ⟨label, _, written⟩ := Option.bind_eq_some_iff.mp remaining
    cases Option.some.inj written
    exact ⟨data, path, row, entry, dataAt, pathAt, rowAt, entryAt, payloadAt, rfl, rfl⟩

theorem owned_source_envelope_identifies_resource_and_occurrence
    {first second : OwnedSourceFacts} (same : first.envelope = second.envelope) :
    first.resource.role = second.resource.role ∧ first.effIndex = second.effIndex := by
  have resource := (List.cons.inj same).1
  have high := (List.cons.inj (List.cons.inj same).2).1
  have low := (List.cons.inj (List.cons.inj (List.cons.inj same).2).2).1
  exact ⟨resource, by omega⟩

theorem decoded_owned_source_facts_bind_wire_columns
    {image : RustDescriptorImage} {choreo : Choreo} {pathIndex event : Nat}
    {facts : OwnedSourceFacts}
    (decoded : image.ownedSourceFacts? choreo pathIndex event = some facts) :
    image.eventEffIndex? event = some facts.effIndex ∧
      image.eventLane? event = some facts.lane ∧
      image.eventFrameLabel? event = some facts.frameLabel := by
  unfold RustDescriptorImage.ownedSourceFacts? at decoded
  obtain ⟨data, _, remaining⟩ := Option.bind_eq_some_iff.mp decoded
  obtain ⟨path, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  obtain ⟨row, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  obtain ⟨entry, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  dsimp only at remaining
  cases payloadAt : entry.2.message.payload with
  | wire schema => simp only [payloadAt] at remaining; contradiction
  | endpoint resource =>
    simp only [payloadAt] at remaining
    obtain ⟨eff, effAt, remaining⟩ := Option.bind_eq_some_iff.mp remaining
    obtain ⟨lane, laneAt, remaining⟩ := Option.bind_eq_some_iff.mp remaining
    obtain ⟨label, labelAt, written⟩ := Option.bind_eq_some_iff.mp remaining
    cases Option.some.inj written
    exact ⟨effAt, laneAt, labelAt⟩

end Hibana
