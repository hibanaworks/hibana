section
open Hibana Hibana.EndpointDelegation Hibana.OwnedCallHistory

def jointImages (actor : Nat) : RustDescriptorImage :=
  if actor = 1 then workerImage else ownerImage

def jointResource : EndpointKey := ⟨0, 10⟩
def jointChannel : TransportChannel :=
  { session := loanWire.session, generation := 0, lane := loanWire.lane,
    sender := loanWire.sender, receiver := loanWire.receiver }
def jointFrame : TransportFrame :=
  { channel := jointChannel, sequence := 0, frameLabel := ⟨loanWire.frameLabel, by decide⟩ }
def jointSend : SourceSendAdmission :=
  { image := ownerImage, base := ownerPending, pathIndex := 0,
    event := custodyLoanSend.event, wire := loanWire }
def jointFacts : OwnedSourceFacts := (jointSend.ownedFacts? custodyChoreo).get (by decide)
def jointEntry : SourceAcceptance :=
  { sample := jointSend, channel := jointChannel, event := jointFacts.effIndex,
    mutation := jointOwnerMutation, carrierWitness := jointWitness }
def jointEvents : List OwnedCallEvent :=
  [.construct jointRequest, .poll jointRequest, .accept jointEntry jointRequest jointFrame]
def jointCut : Cut :=
  { requests := [jointRequest], custody :=
      { usable := [], preparing := [], acceptedFutures := [jointRequest.payload],
        inFlight := [⟨jointEntry.callKey, jointFrame⟩] } }
def jointEmpty : Observation Tag Receipt :=
  { usable := [], preparing := [], acceptedFutures := [], inFlight := [] }

/-- This is the constructor, first Pending poll and physical acceptance from
one existing two-runtime execution. No publication or fabricated receiver
value is inserted into this prefix. -/
theorem actualJointConstructorAcceptance :
    History jointImages jointResource custodyChoreo initialValue jointEvents jointCut := by
  have moved : History jointImages jointResource custodyChoreo initialValue
      [.construct jointRequest] ⟨[jointRequest],
        { usable := [], preparing := [jointRequest.payload], acceptedFutures := [], inFlight := [] }⟩ := by
    have same : initialValue = jointRequest.payload := by decide
    rw [same]
    exact .tail .nil (.construct [] (EndpointTransfer.initial jointRequest.payload) [] [] [] jointRequest)
  have pending : History jointImages jointResource custodyChoreo initialValue
      [.construct jointRequest, .poll jointRequest] ⟨[jointRequest],
        { usable := [], preparing := [jointRequest.payload], acceptedFutures := [], inFlight := [] }⟩ :=
    History.tail moved (Boundary.poll _ _ jointRequest (by decide))
  refine History.tail pending (Boundary.accept _ jointEmpty [] [] [jointRequest]
    jointEntry jointRequest jointFrame jointFacts (Option.some_get _).symm
    ?_ ?_ ?_ ?_ ?_ ?_ ?_)
  · decide
  · decide
  · decide
  · unfold Request.matches; decide
  · unfold OwnedWireObservation.identifies; decide
  · rfl
  · decide

def jointSourceInitial : DistributedSourceCut := .initial fun actor =>
  if actor = 1 then (workerBefore, jointWorkerMutation) else (ownerPending, jointOwnerMutation)
def jointSourceAfter : DistributedSourceCut := jointSourceInitial.carrierAt jointChannel
  (((jointSourceInitial.carriers jointChannel).send ⟨loanWire.frameLabel, by decide⟩).get (by decide))

theorem actualJointSourceAcceptance : DistributedCallExecution jointImages custodyChoreo 873
    jointSourceInitial jointSourceAfter (sourceWord jointEvents) [jointEntry] [] [jointEntry] := by
  change DistributedCallExecution jointImages custodyChoreo 873
    jointSourceInitial jointSourceAfter [.accept jointEntry] [jointEntry] [] [jointEntry]
  apply DistributedCallExecution.accept (sample := jointSend) (previousPath := 0)
    (first := jointSourceInitial) (cut := jointSourceInitial)
    (word := []) (entries := []) (published := []) (pending := [])
    (channel := jointChannel) (event := jointFacts.effIndex)
    (witness := jointWitness) (label := ⟨loanWire.frameLabel, by decide⟩)
    (data := (ownerImage.decodeActorOrder? custodyChoreo).get (by decide))
    (path := ((ownerImage.decodeActorOrder? custodyChoreo).get (by decide)).normalized.paths[0])
    (row := ((ownerImage.decodeActorOrder? custodyChoreo).get (by decide)).rows[custodyLoanSend.event])
  · exact .refl
  · rfl
  · decide
  · exact (Option.some_get _).symm
  · exact getElem?_pos _ _ (by decide)
  · exact getElem?_pos _ _ (by decide)
  · exact .current (by decide)
  · decide
  · decide
  · decide
  · decide
  · unfold OwnedWireObservation.identifies; decide

theorem actualJointQueuedOrigin :
    ∃ earlier later entry request facts,
      jointEvents = earlier ++ OwnedCallEvent.accept entry request jointFrame :: later ∧
      entry ∈ [jointEntry] ∧ entry.callKey = jointEntry.callKey ∧
      OwnedCallEvent.construct request ∈ earlier ∧
      entry.sample.image = jointImages entry.sample.image.role ∧
      entry.sample.check custodyChoreo 873 = true ∧
      entry.sample.ownedFacts? custodyChoreo = some facts ∧ facts.resource = jointResource ∧
      facts.side = .send ∧ request.matches entry.sample facts ∧
      entry.sample.wire.identifies jointFrame ∧ jointFrame.channel = entry.channel ∧
      jointFrame.sequence = (sourceChannelEntries (acceptedEntries earlier) entry.channel).length :=
  a_joint_queued_transfer_has_its_checked_source_and_actual_constructor
    (receipt := ⟨jointEntry.callKey, jointFrame⟩)
    actualJointConstructorAcceptance actualJointSourceAcceptance (by decide)
end
