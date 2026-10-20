import Hibana.SourcePortWord

namespace Hibana
open EndpointDelegation

private theorem owned_source_facts_retain_identity_and_payload
    {image : RustDescriptorImage} {choreo : Choreo} {pathIndex event : Nat}
    {facts : OwnedSourceFacts}
    (decoded : image.ownedSourceFacts? choreo pathIndex event = some facts) :
    sourceOperationIdentity? image choreo pathIndex event =
        some (facts.effIndex, facts.binding, facts.side) ∧
      facts.binding.message.payload = .endpoint facts.resource := by
  obtain ⟨data, path, row, entry, dataAt, pathAt, rowAt, entryAt, payloadAt, sideAt, bindingAt⟩ :=
    decoded_owned_source_facts_retain_the_exact_source_row decoded
  have effAt := (decoded_owned_source_facts_bind_wire_columns decoded).1
  constructor
  · simp only [sourceOperationIdentity?, Option.bind_eq_bind, dataAt, pathAt, rowAt,
      entryAt, effAt, Option.bind_some, Option.pure_def, sideAt, ← bindingAt]
  · exact bindingAt ▸ payloadAt

/-- The common source table fixes the logical message of matched operations,
including ordinary payloads. Only immutable row identities are compared;
physical holders, selected paths and visit histories need not be equal. -/
theorem source_matched_operations_have_the_same_logical_message
    {sentImage receivedImage : RustDescriptorImage} {choreo : Choreo}
    {sentPath receivedPath sentEvent receivedEvent source : Nat}
    {sentBinding receivedBinding : Binding} {sentSide receivedSide : Side}
    {atoms : List DecodedProgramAtom}
    (sentIdentity : sourceOperationIdentity? sentImage choreo sentPath sentEvent =
      some (source, sentBinding, sentSide))
    (receivedIdentity : sourceOperationIdentity? receivedImage choreo receivedPath receivedEvent =
      some (source, receivedBinding, receivedSide))
    (sentAtoms : sentImage.decodeAtoms? = some atoms)
    (receivedAtoms : receivedImage.decodeAtoms? = some atoms) :
    sentBinding.message = receivedBinding.message := by
  unfold sourceOperationIdentity? at sentIdentity receivedIdentity
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq] at sentIdentity receivedIdentity
  obtain ⟨leftData, leftAt, leftPath, leftPathAt, leftRow, leftRowAt,
    leftEntry, leftEntryAt, sentEff, sentEffAt, leftSame⟩ := sentIdentity
  obtain ⟨rightData, rightAt, rightPath, rightPathAt, rightRow, rightRowAt,
    rightEntry, rightEntryAt, receivedEff, receivedEffAt, rightSame⟩ := receivedIdentity
  have leftRows := (decoded_actor_order_fields leftAt).2.2.2.2.1
  have rightRows := (decoded_actor_order_fields rightAt).2.2.2.2.1
  have leftIndex := decoded_actor_row_lookup_preserves_its_index leftRows leftRowAt
  have rightIndex := decoded_actor_row_lookup_preserves_its_index rightRows rightRowAt
  obtain ⟨leftEff, leftAtom, leftEffAt, leftSource⟩ :=
    decoded_actor_row_retains_its_unique_source_position leftRows sentAtoms (List.mem_of_getElem? leftRowAt)
  obtain ⟨rightEff, rightAtom, rightEffAt, rightSource⟩ :=
    decoded_actor_row_retains_its_unique_source_position rightRows receivedAtoms (List.mem_of_getElem? rightRowAt)
  rw [leftIndex] at leftEffAt
  rw [rightIndex] at rightEffAt
  have leftEffect : leftEff = source :=
    (Option.some.inj (leftEffAt.symm.trans sentEffAt)).trans (congrArg Prod.fst leftSame)
  have rightEffect : rightEff = source :=
    (Option.some.inj (rightEffAt.symm.trans receivedEffAt)).trans (congrArg Prod.fst rightSame)
  have sameOffset : leftEff = rightEff := leftEffect.trans rightEffect.symm
  rw [sameOffset] at leftSource
  have sameOccurrence : leftRow.actionId.event = rightRow.actionId.event :=
    congrArg Prod.snd (Option.some.inj (leftSource.symm.trans rightSource))
  have leftOrigin := decoded_actor_order_retains_its_normalized_source leftAt
  have rightOrigin := decoded_actor_order_retains_its_normalized_source rightAt
  have sameSource := Option.some.inj (leftOrigin.1.symm.trans rightOrigin.1)
  have sameNormalized : leftData.normalized = rightData.normalized := by
    rw [sameSource] at leftOrigin
    exact Option.some.inj (leftOrigin.2.symm.trans rightOrigin.2)
  have leftKey : leftEntry.1 = leftRow.actionId.event := beq_iff_eq.mp
    (List.find?_some (p := fun entry : Nat × Binding => entry.1 == leftRow.actionId.event) leftEntryAt)
  have rightKey : rightEntry.1 = rightRow.actionId.event := beq_iff_eq.mp
    (List.find?_some (p := fun entry : Nat × Binding => entry.1 == rightRow.actionId.event) rightEntryAt)
  have leftMember : (rightRow.actionId.event, leftEntry.2) ∈ leftPath.bindings := by
    rw [← sameOccurrence, ← leftKey]
    exact List.mem_of_find?_eq_some leftEntryAt
  have rightMember : (rightRow.actionId.event, rightEntry.2) ∈ rightPath.bindings := by
    rw [← rightKey]
    exact List.mem_of_find?_eq_some rightEntryAt
  have selectedLeft : leftPath ∈ rightData.normalized.paths := by
    rw [← sameNormalized]
    exact List.mem_of_getElem? leftPathAt
  have message := normalized_paths_agree_on_a_shared_source_message rightOrigin.2
    selectedLeft (List.mem_of_getElem? rightPathAt) leftMember rightMember
  have leftBinding : leftEntry.2 = sentBinding :=
    congrArg (fun triple : Nat × Binding × Side => triple.2.1) leftSame
  have rightBinding : rightEntry.2 = receivedBinding :=
    congrArg (fun triple : Nat × Binding × Side => triple.2.1) rightSame
  simpa only [leftBinding, rightBinding] using message

/-- Matching offsets in the shared immutable source table determine the
logical message on independently selected paths. Physical holders, unknown
sibling choices and repetition histories need not agree. This is a source
translation result; it does not assert possession at either actor. -/
theorem source_matched_owned_halves_have_the_same_logical_message
    {sentImage receivedImage : RustDescriptorImage} {choreo : Choreo}
    {sentPath receivedPath sentEvent receivedEvent : Nat}
    {sentFacts receivedFacts : OwnedSourceFacts} {atoms : List DecodedProgramAtom}
    (sentAt : sentImage.ownedSourceFacts? choreo sentPath sentEvent = some sentFacts)
    (receivedAt : receivedImage.ownedSourceFacts? choreo receivedPath receivedEvent = some receivedFacts)
    (sentAtoms : sentImage.decodeAtoms? = some atoms)
    (receivedAtoms : receivedImage.decodeAtoms? = some atoms)
    (sameEffect : sentFacts.effIndex = receivedFacts.effIndex) :
    sentFacts.binding.message = receivedFacts.binding.message ∧
      sentFacts.resource = receivedFacts.resource := by
  have sent := owned_source_facts_retain_identity_and_payload sentAt
  have received := owned_source_facts_retain_identity_and_payload receivedAt
  rw [sameEffect] at sent
  have message := source_matched_operations_have_the_same_logical_message
    sent.1 received.1 sentAtoms receivedAtoms
  have payloadSame : Payload.endpoint sentFacts.resource = .endpoint receivedFacts.resource :=
    sent.2.symm.trans ((congrArg Message.payload message).trans received.2)
  exact ⟨message, Payload.endpoint.inj payloadSame⟩

/-- Classification of a general accepted send comes from its source message.
The exact resource is derived; a caller cannot supply an unrelated owned kind. -/
theorem a_checked_sender_with_owned_message_has_its_source_facts
    {sender : SourceSendAdmission} {choreo : Choreo} {session source : Nat}
    {binding : Binding} {side : Side} {resource : EndpointKey}
    (accepted : sender.check choreo session = true)
    (identity : sourceOperationIdentity? sender.image choreo sender.pathIndex sender.event =
      some (source, binding, side))
    (payload : binding.message.payload = .endpoint resource) :
    ∃ facts, sender.ownedFacts? choreo = some facts ∧ facts.resource = resource := by
  have originalIdentity := identity
  unfold sourceOperationIdentity? at identity
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq] at identity
  obtain ⟨data, decoded, path, pathAt, row, rowAt, entry, entryAt, eff, _, same⟩ := identity
  have bindingSame : entry.2 = binding :=
    congrArg (fun triple : Nat × Binding × Side => triple.2.1) same
  have sourcePayload : entry.2.message.payload = .endpoint resource := bindingSame ▸ payload
  unfold SourceSendAdmission.check at accepted
  have rest := (Bool.and_eq_true_iff.mp accepted).2
  simp only [decoded, pathAt, rowAt, entryAt] at rest
  have payloadChecked := (Bool.and_eq_true_iff.mp rest).2
  simp only [SourceSendAdmission.payloadMatches, sourcePayload] at payloadChecked
  cases factsAt : sender.ownedFacts? choreo with
  | none => simp only [factsAt, Bool.false_eq_true] at payloadChecked
  | some facts =>
    have actual := owned_source_facts_retain_identity_and_payload factsAt
    have bindingSame : facts.binding = binding :=
      congrArg (fun triple : Nat × Binding × Side => triple.2.1)
        (Option.some.inj (actual.1.symm.trans originalIdentity))
    have actualPayload : binding.message.payload = .endpoint facts.resource := by
      simpa only [bindingSame] using actual.2
    exact ⟨facts, rfl, Payload.endpoint.inj (actualPayload.symm.trans payload)⟩

private theorem owned_wire_check_from_its_source_message
    {sender : SourceSendAdmission} {receiverImage : RustDescriptorImage}
    {choreo : Choreo} {session pathIndex event : Nat}
    {sentFacts receivedFacts : OwnedSourceFacts} {wire : OwnedWireObservation}
    {frame : TransportFrame}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (receivedAt : receiverImage.ownedSourceFacts? choreo pathIndex event = some receivedFacts)
    (resourceSame : receivedFacts.resource = sentFacts.resource)
    (effectSame : receivedFacts.effIndex = sentFacts.effIndex)
    (messageSame : receivedFacts.binding.message = sentFacts.binding.message)
    (actorAt : receivedFacts.actor? = some receiverImage.role)
    (acceptedFrame : sender.wire.identifies frame) (receivedFrame : wire.identifies frame)
    (laneAt : receiverImage.eventLane? event = some wire.lane)
    (colorAt : receiverImage.eventFrameLabel? event = some wire.frameLabel) :
    receivedFacts.wireMatches receiverImage.role wire session = true ↔
      wire.payload = sender.wire.payload := by
  have checked := (checked_source_send_has_its_owned_payload accepted sentAt).2
  have fields : sentFacts.resource.component = 0 ∧ sentFacts.resource.role < 256 ∧
      sentFacts.effIndex < 65536 ∧ sentFacts.actor? = some sender.base.actor ∧
      sender.wire.session = session ∧ sender.wire.lane = sentFacts.lane ∧
      sender.wire.frameLabel = sentFacts.frameLabel ∧
      sender.wire.sender = sentFacts.binding.message.sender.role ∧
      sender.wire.receiver = sentFacts.binding.message.receiver.role ∧
      sender.wire.payload = sentFacts.envelope := by
    simpa only [OwnedSourceFacts.wireMatches, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq, and_assoc] using checked
  have columns := decoded_owned_source_facts_bind_wire_columns receivedAt
  have lane : wire.lane = receivedFacts.lane := Option.some.inj (laneAt.symm.trans columns.2.1)
  have color : wire.frameLabel = receivedFacts.frameLabel := Option.some.inj (colorAt.symm.trans columns.2.2)
  have sessionAt : wire.session = session := receivedFrame.1.trans (acceptedFrame.1.symm.trans fields.2.2.2.2.1)
  have fromAt : wire.sender = receivedFacts.binding.message.sender.role := by
    rw [messageSame]
    exact receivedFrame.2.2.1.trans (acceptedFrame.2.2.1.symm.trans fields.2.2.2.2.2.2.2.1)
  have toAt : wire.receiver = receivedFacts.binding.message.receiver.role := by
    rw [messageSame]
    exact receivedFrame.2.2.2.1.trans (acceptedFrame.2.2.2.1.symm.trans fields.2.2.2.2.2.2.2.2.1)
  have envelope : receivedFacts.envelope = sender.wire.payload := by
    rw [fields.2.2.2.2.2.2.2.2.2]
    simp only [OwnedSourceFacts.envelope, resourceSame, effectSame]
  simp only [OwnedSourceFacts.wireMatches, Bool.and_eq_true, beq_iff_eq, decide_eq_true_eq, and_assoc]
  have component : receivedFacts.resource.component = 0 := resourceSame ▸ fields.1
  have roleBound : receivedFacts.resource.role < 256 := resourceSame ▸ fields.2.1
  have eventBound : receivedFacts.effIndex < 65536 := effectSame ▸ fields.2.2.1
  simp only [component, roleBound, eventBound, actorAt, sessionAt, lane, color, fromAt, toAt,
    envelope, eq_self, true_and]

/-- Before receipt or receiver publication, matching source operations fixes
all owned-envelope checks. Once the ordinary header/descriptor checks pass,
the remaining test accepts exactly the bytes of the actual accepted sender.
There is no successful receiver snapshot, current receive-pair check, shared
visit, or assumption that a raw header is an application-owned value.
Carrier payload preservation is a consequence to check at an integration,
not inferred merely because two observations identify the same frame header. -/
theorem a_source_matched_owned_head_has_its_exact_receive_wire_check
    {sender : SourceSendAdmission} {receiverImage : RustDescriptorImage}
    {choreo : Choreo} {session source pathIndex event : Nat} {binding : Binding}
    {sentFacts receivedFacts : OwnedSourceFacts} {wire : OwnedWireObservation}
    {frame : TransportFrame}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (receivedAt : receiverImage.ownedSourceFacts? choreo pathIndex event = some receivedFacts)
    (sendIdentity : sourceOperationIdentity? sender.image choreo sender.pathIndex sender.event =
      some (source, binding, .send))
    (receiveIdentity : sourceOperationIdentity? receiverImage choreo pathIndex event =
      some (source, binding, .receive))
    (physical : receiverImage.role = binding.receiverActor)
    (acceptedFrame : sender.wire.identifies frame) (receivedFrame : wire.identifies frame)
    (laneAt : receiverImage.eventLane? event = some wire.lane)
    (colorAt : receiverImage.eventFrameLabel? event = some wire.frameLabel) :
    receivedFacts.resource = sentFacts.resource ∧ receivedFacts.effIndex = sentFacts.effIndex ∧
      receivedFacts.side = .receive ∧
      (receivedFacts.wireMatches receiverImage.role wire session = true ↔
        wire.payload = sender.wire.payload) := by
  have sent := owned_source_facts_retain_identity_and_payload sentAt
  have received := owned_source_facts_retain_identity_and_payload receivedAt
  have sendSame := Option.some.inj (sent.1.symm.trans sendIdentity)
  have receiveSame := Option.some.inj (received.1.symm.trans receiveIdentity)
  have sendEff : sentFacts.effIndex = source := congrArg Prod.fst sendSame
  have recvEff : receivedFacts.effIndex = source := congrArg Prod.fst receiveSame
  have sendBinding : sentFacts.binding = binding := congrArg (fun triple : Nat × Binding × Side => triple.2.1) sendSame
  have recvBinding : receivedFacts.binding = binding := congrArg (fun triple : Nat × Binding × Side => triple.2.1) receiveSame
  have recvSide : receivedFacts.side = .receive := congrArg (fun triple : Nat × Binding × Side => triple.2.2) receiveSame
  have samePayload : Payload.endpoint receivedFacts.resource = .endpoint sentFacts.resource := by
    rw [recvBinding] at received
    rw [sendBinding] at sent
    exact received.2.symm.trans sent.2
  have resourceSame := Payload.endpoint.inj samePayload
  have effectSame := recvEff.trans sendEff.symm
  have actorAt : receivedFacts.actor? = some receiverImage.role := by
    simp only [OwnedSourceFacts.actor?, recvSide, recvBinding, physical]
  have messageSame : receivedFacts.binding.message = sentFacts.binding.message := by
    rw [recvBinding, sendBinding]
  exact ⟨resourceSame, effectSame, recvSide,
    owned_wire_check_from_its_source_message accepted sentAt receivedAt resourceSame effectSame
      messageSame actorAt acceptedFrame receivedFrame laneAt colorAt⟩

/-- A source-matched owned receive can check arbitrary bytes without equating
the participants' selected paths or physical holder bindings. The shared atom
table and effect identity derive the logical message/resource; receiver actor,
lane and color are immutable local projection facts. No receive commit,
fresh value, current receive-pair or common repetition visit is an input. -/
theorem a_source_matched_owned_head_checks_bytes_on_independent_paths
    {sender : SourceSendAdmission} {receiverImage : RustDescriptorImage}
    {choreo : Choreo} {session pathIndex event : Nat}
    {sentFacts receivedFacts : OwnedSourceFacts} {atoms : List DecodedProgramAtom}
    {wire : OwnedWireObservation} {frame : TransportFrame}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (receivedAt : receiverImage.ownedSourceFacts? choreo pathIndex event = some receivedFacts)
    (sentAtoms : sender.image.decodeAtoms? = some atoms)
    (receivedAtoms : receiverImage.decodeAtoms? = some atoms)
    (sameEffect : sentFacts.effIndex = receivedFacts.effIndex)
    (receiving : receivedFacts.side = .receive)
    (physical : receivedFacts.binding.receiverActor = receiverImage.role)
    (acceptedFrame : sender.wire.identifies frame) (receivedFrame : wire.identifies frame)
    (laneAt : receiverImage.eventLane? event = some wire.lane)
    (colorAt : receiverImage.eventFrameLabel? event = some wire.frameLabel) :
    receivedFacts.resource = sentFacts.resource ∧
      (receivedFacts.wireMatches receiverImage.role wire session = true ↔
        wire.payload = sender.wire.payload) := by
  obtain ⟨messageSame, resourceSame⟩ := source_matched_owned_halves_have_the_same_logical_message
    sentAt receivedAt sentAtoms receivedAtoms sameEffect
  have actorAt : receivedFacts.actor? = some receiverImage.role := by
    simp only [OwnedSourceFacts.actor?, receiving, physical]
  exact ⟨resourceSame.symm, owned_wire_check_from_its_source_message accepted sentAt receivedAt
    resourceSame.symm sameEffect.symm messageSame.symm actorAt acceptedFrame receivedFrame laneAt colorAt⟩

/-- A prospective owned receive derives the accepted sender's payload kind
from their source correspondence. The sender initially carries an ordinary
or owned frame; no sender owned-facts witness is an input. Classification
comes from the shared source message, never from received payload bytes.
The same result supplies the exact wire check before the receive commits. -/
theorem a_matched_owned_receive_recovers_its_accepted_sender
    {sender : SourceSendAdmission} {receiverImage : RustDescriptorImage}
    {choreo : Choreo} {session pathIndex event : Nat}
    {receivedFacts : OwnedSourceFacts} {atoms : List DecodedProgramAtom}
    {wire : OwnedWireObservation} {frame : TransportFrame}
    (accepted : sender.check choreo session = true)
    (receivedAt : receiverImage.ownedSourceFacts? choreo pathIndex event = some receivedFacts)
    (sentAtoms : sender.image.decodeAtoms? = some atoms)
    (receivedAtoms : receiverImage.decodeAtoms? = some atoms)
    (sourceAt : sender.effIndex? = some receivedFacts.effIndex)
    (receiving : receivedFacts.side = .receive)
    (physical : receivedFacts.binding.receiverActor = receiverImage.role)
    (acceptedFrame : sender.wire.identifies frame) (receivedFrame : wire.identifies frame)
    (laneAt : receiverImage.eventLane? event = some wire.lane)
    (colorAt : receiverImage.eventFrameLabel? event = some wire.frameLabel) :
    ∃ sentFacts, sender.ownedFacts? choreo = some sentFacts ∧
      sentFacts.resource = receivedFacts.resource ∧ sentFacts.side = .send ∧
      (receivedFacts.wireMatches receiverImage.role wire session = true ↔
        wire.payload = sender.wire.payload) := by
  obtain ⟨source, binding, identity, effectAt, _⟩ :=
    checked_source_send_has_its_bound_wire_columns accepted
  have sameSource := Option.some.inj (effectAt.symm.trans sourceAt)
  rw [sameSource] at identity
  have receiver := owned_source_facts_retain_identity_and_payload receivedAt
  have message := source_matched_operations_have_the_same_logical_message
    identity receiver.1 sentAtoms receivedAtoms
  have payload : binding.message.payload = .endpoint receivedFacts.resource :=
    (congrArg Message.payload message).trans receiver.2
  obtain ⟨sentFacts, sentAt, _⟩ := a_checked_sender_with_owned_message_has_its_source_facts
    accepted identity payload
  have sameEffect : sentFacts.effIndex = receivedFacts.effIndex := Option.some.inj
    ((decoded_owned_source_facts_bind_wire_columns sentAt).1.symm.trans sourceAt)
  have checked := a_source_matched_owned_head_checks_bytes_on_independent_paths
    accepted sentAt receivedAt sentAtoms receivedAtoms sameEffect receiving physical
    acceptedFrame receivedFrame laneAt colorAt
  exact ⟨sentFacts, sentAt, checked.1.symm,
    (checked_source_send_has_its_owned_payload accepted sentAt).1, checked.2⟩

end Hibana
