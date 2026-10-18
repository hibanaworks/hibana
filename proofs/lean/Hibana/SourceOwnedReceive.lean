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
    rw [recvBinding, ← sendBinding]
    exact receivedFrame.2.2.1.trans (acceptedFrame.2.2.1.symm.trans fields.2.2.2.2.2.2.2.1)
  have toAt : wire.receiver = receivedFacts.binding.message.receiver.role := by
    rw [recvBinding, ← sendBinding]
    exact receivedFrame.2.2.2.1.trans (acceptedFrame.2.2.2.1.symm.trans fields.2.2.2.2.2.2.2.2.1)
  have actorAt : receivedFacts.actor? = some receiverImage.role := by
    simp only [OwnedSourceFacts.actor?, recvSide, recvBinding, physical]
  have envelope : receivedFacts.envelope = sender.wire.payload := by
    rw [fields.2.2.2.2.2.2.2.2.2]
    simp only [OwnedSourceFacts.envelope, resourceSame, effectSame]
  refine ⟨resourceSame, effectSame, recvSide, ?_⟩
  simp only [OwnedSourceFacts.wireMatches, Bool.and_eq_true, beq_iff_eq, decide_eq_true_eq, and_assoc]
  have component : receivedFacts.resource.component = 0 := resourceSame ▸ fields.1
  have roleBound : receivedFacts.resource.role < 256 := resourceSame ▸ fields.2.1
  have eventBound : receivedFacts.effIndex < 65536 := effectSame ▸ fields.2.2.1
  simp only [component, roleBound, eventBound, actorAt, sessionAt, lane, color, fromAt, toAt,
    envelope, eq_self, true_and]

end Hibana
