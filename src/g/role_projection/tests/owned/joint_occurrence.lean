section
open Hibana Hibana.EndpointDelegation Hibana.OwnedCallHistory

def jointOccurrenceSource : Source := (custodyChoreo.ownershipSource?).get (by decide)
def jointOccurrenceOriginal : Normalization := (normalize jointOccurrenceSource).get (by decide)
def jointOccurrenceNormalized : Normalization :=
  (normalize jointOccurrenceSource.oneCopy).get (by decide)
def jointOccurrencePlan : SourceOccurrenceCut jointOccurrenceSource := {
  original := jointOccurrenceOriginal
  originalAccepted := (Option.some_get _).symm
  expanded := jointOccurrenceSource.oneCopy
  unfolding := SourceUnfolding.oneCopy _
  normalized := jointOccurrenceNormalized
  accepted := (Option.some_get _).symm
  sameIssuance := by decide
  path := jointOccurrenceNormalized.paths[0]
  selected := by decide
  observed := fun _ => false
}

def jointLoanRecord : ActorCommitRecord := ⟨custodyLoanSend, loanWire⟩
def jointLoanReceipt : ActorCommitRecord := ⟨custodyLoanRecv, loanWire⟩
def jointReturnRecord : ActorCommitRecord := ⟨custodyReturnSend, returnWire⟩
def jointReturnReceipt : ActorCommitRecord := ⟨custodyReturnRecv, returnWire⟩
def jointReturnChannel : TransportChannel :=
  { session := returnWire.session, generation := 0, lane := returnWire.lane,
    sender := returnWire.sender, receiver := returnWire.receiver }
def jointReturnFrame : TransportFrame :=
  { channel := jointReturnChannel, sequence := 0, frameLabel := ⟨returnWire.frameLabel, by decide⟩ }
def jointReturnSend : SourceSendAdmission :=
  { image := workerImage, base := workerSealed, pathIndex := 0,
    event := custodyReturnSend.event, wire := returnWire }
def jointReturnFacts : OwnedSourceFacts := (jointReturnSend.ownedFacts? custodyChoreo).get (by decide)
def jointReturnEntry : SourceAcceptance :=
  { sample := jointReturnSend, channel := jointReturnChannel, event := jointReturnFacts.effIndex,
    mutation := jointWorkerReceivedMutation, carrierWitness := jointReturnWitness }
def jointLoanReceiptFacts : OwnedCommitFacts :=
  (custodyLoanRecv.ownedFacts? workerImage custodyChoreo).get (by decide)
def jointReturnReceiptFacts : OwnedCommitFacts :=
  (custodyReturnRecv.ownedFacts? ownerImage custodyChoreo).get (by decide)

/-- One chronology, including real constructor/poll/Drop records and the exact
two accepted carrier witnesses. The sender publishes before its Ready poll is
recorded; each completed future remains present through the peer's receipt. -/
def jointRoundTripEvents : List OwnedCallEvent := jointEvents ++ [
  .source (.publish jointEntry.callKey 0 jointLoanRecord), .poll jointRequest,
  .receive jointEntry.callKey 1 jointLoanReceipt jointFrame receivedValue,
  .construct jointReturnRequest, .accept jointReturnEntry jointReturnRequest jointReturnFrame,
  .source (.publish jointReturnEntry.callKey 1 jointReturnRecord), .poll jointReturnRequest,
  .receive jointReturnEntry.callKey 0 jointReturnReceipt jointReturnFrame returnedValue,
  .retire jointReturnRequest, .retire jointRequest]

def jointLoanReceivedCut : Cut :=
  { requests := [jointRequest], custody :=
    { usable := [receivedValue], preparing := [], acceptedFutures := [initialValue], inFlight := [] } }
def jointReturnPreparedCut : Cut :=
  { requests := [jointReturnRequest, jointRequest], custody :=
    { usable := [], preparing := [receivedValue], acceptedFutures := [initialValue], inFlight := [] } }
def jointReturnFlightCut : Cut :=
  { requests := [jointReturnRequest, jointRequest], custody :=
    { usable := [], preparing := [], acceptedFutures := [receivedValue, initialValue],
      inFlight := [⟨jointReturnEntry.callKey, jointReturnFrame⟩] } }
def jointReturnReceivedCut : Cut :=
  { requests := [jointReturnRequest, jointRequest], custody :=
    { usable := [returnedValue], preparing := [], acceptedFutures := [receivedValue, initialValue], inFlight := [] } }
def jointReturnRetiredCut : Cut :=
  { requests := [jointRequest], custody :=
    { usable := [returnedValue], preparing := [], acceptedFutures := [initialValue], inFlight := [] } }
def jointReturnedCut : Cut :=
  { requests := [], custody :=
    { usable := [returnedValue], preparing := [], acceptedFutures := [], inFlight := [] } }

theorem actualJointRoundTripCustody :
    History jointImages jointResource custodyChoreo initialValue jointRoundTripEvents jointReturnedCut := by
  have h1 := History.tail actualJointConstructorAcceptance
    (Boundary.source _ _ (.publish jointEntry.callKey 0 jointLoanRecord) trivial)
  have h2 := History.tail h1 (Boundary.poll _ _ jointRequest (by decide))
  have h3 : History jointImages jointResource custodyChoreo initialValue
      (jointRoundTripEvents.take 6) jointLoanReceivedCut := by
    refine History.tail h2 (Boundary.receive _
      { jointEmpty with acceptedFutures := [initialValue] } [] [jointRequest]
      jointEntry.callKey 1 jointLoanReceipt jointFrame jointLoanReceiptFacts
      (Option.some_get _).symm ?_ ?_ ?_ ?_)
    · decide
    · decide
    · decide
    · unfold OwnedWireObservation.identifies; decide
  have h4 : History jointImages jointResource custodyChoreo initialValue
      (jointRoundTripEvents.take 7) jointReturnPreparedCut :=
    History.tail h3 (Boundary.construct _
      { jointEmpty with acceptedFutures := [initialValue] } [] [] [jointRequest] jointReturnRequest)
  have h5 : History jointImages jointResource custodyChoreo initialValue
      (jointRoundTripEvents.take 8) jointReturnFlightCut := by
    refine History.tail h4 (Boundary.accept _
      { jointEmpty with acceptedFutures := [initialValue] } [] [] [jointReturnRequest, jointRequest]
      jointReturnEntry jointReturnRequest jointReturnFrame jointReturnFacts
      (Option.some_get _).symm ?_ ?_ ?_ ?_ ?_ ?_ ?_)
    · decide
    · decide
    · decide
    · unfold Request.matches; decide
    · unfold OwnedWireObservation.identifies; decide
    · rfl
    · decide
  have h6 := History.tail h5
    (Boundary.source _ _ (.publish jointReturnEntry.callKey 1 jointReturnRecord) trivial)
  have h7 := History.tail h6 (Boundary.poll _ _ jointReturnRequest (by decide))
  have h8 : History jointImages jointResource custodyChoreo initialValue
      (jointRoundTripEvents.take 11) jointReturnReceivedCut := by
    refine History.tail h7 (Boundary.receive _
      { jointEmpty with acceptedFutures := [receivedValue, initialValue] } [] [jointReturnRequest, jointRequest]
      jointReturnEntry.callKey 0 jointReturnReceipt jointReturnFrame jointReturnReceiptFacts
      (Option.some_get _).symm ?_ ?_ ?_ ?_)
    · decide
    · decide
    · decide
    · unfold OwnedWireObservation.identifies; decide
  have h9 : History jointImages jointResource custodyChoreo initialValue
      (jointRoundTripEvents.take 12) jointReturnRetiredCut :=
    History.tail h8 (Boundary.retire _
      { jointEmpty with usable := [returnedValue] } [] [initialValue] [] [jointRequest] jointReturnRequest)
  have h10 : History jointImages jointResource custodyChoreo initialValue
      jointRoundTripEvents jointReturnedCut :=
    History.tail h9 (Boundary.retire _
      { jointEmpty with usable := [returnedValue] } [] [] [] [] jointRequest)
  simpa only [jointRoundTripEvents, List.append_assoc, List.cons_append, List.nil_append] using h10

def jointSourcePublished : DistributedSourceCut :=
  jointSourceAfter.actorAt 0 (ownerSent, jointOwnerSentMutation)
def jointSourceLoanReceived : DistributedSourceCut :=
  (jointSourcePublished.actorAt 1 (workerReceived, jointWorkerReceivedMutation)).carrierAt jointChannel
    { jointSourcePublished.carriers jointChannel with delivered := 1 }
def jointSourceReturnAccepted : DistributedSourceCut :=
  jointSourceLoanReceived.carrierAt jointReturnChannel
    (((jointSourceLoanReceived.carriers jointReturnChannel).send
      ⟨returnWire.frameLabel, by decide⟩).get (by decide))
def jointSourceReturnPublished : DistributedSourceCut :=
  jointSourceReturnAccepted.actorAt 1 (workerSent, jointWorkerSentMutation)
def jointSourceReturned : DistributedSourceCut :=
  (jointSourceReturnPublished.actorAt 0 (ownerReturned, jointOwnerReturnedMutation)).carrierAt jointReturnChannel
    { jointSourceReturnPublished.carriers jointReturnChannel with delivered := 1 }

theorem actualJointRoundTripSource : DistributedCallExecution jointImages custodyChoreo 873
    jointSourceInitial jointSourceReturned (sourceWord jointRoundTripEvents)
    [jointEntry, jointReturnEntry] [(jointEntry, jointLoanRecord), (jointReturnEntry, jointReturnRecord)] [] := by
  have h1 : DistributedCallExecution jointImages custodyChoreo 873 jointSourceInitial jointSourcePublished
      ((sourceWord jointRoundTripEvents).take 2) [jointEntry] [(jointEntry, jointLoanRecord)] [] := by
    apply DistributedCallExecution.publish (left := []) (right := []) (entry := jointEntry)
      (actor := 0) (record := jointLoanRecord) (after := (ownerSent, jointOwnerSentMutation))
      actualJointSourceAcceptance (by decide) (by decide)
    exact ActorMeasuredCommit.ordinary (record := ⟨jointLoanRecord, jointOwnerMutation, jointOwnerSentMutation⟩)
      (by decide) (by decide)
  have h2 : DistributedCallExecution jointImages custodyChoreo 873 jointSourceInitial jointSourceLoanReceived
      ((sourceWord jointRoundTripEvents).take 3) [jointEntry] [(jointEntry, jointLoanRecord)] [] := by
    apply DistributedCallExecution.receive (entry := jointEntry) (frame := jointFrame)
      (witness := jointWitness) (actor := 1) (record := jointLoanReceipt)
      (channel := jointChannel) (after := (workerReceived, jointWorkerReceivedMutation))
      (next := { jointSourcePublished.carriers jointChannel with delivered := 1 }) h1
    · decide
    · decide
    · decide
    · rfl
    · exact a_concrete_receive_pair_has_coherent_local_witnesses (by decide)
    · exact ActorMeasuredCommit.ordinary
        (record := ⟨jointLoanReceipt, jointWorkerMutation, jointWorkerReceivedMutation⟩) (by decide) (by decide)
  have h3 : DistributedCallExecution jointImages custodyChoreo 873 jointSourceInitial jointSourceReturnAccepted
      ((sourceWord jointRoundTripEvents).take 4)
      [jointEntry, jointReturnEntry] [(jointEntry, jointLoanRecord)] [jointReturnEntry] := by
    apply DistributedCallExecution.accept (sample := jointReturnSend) (previousPath := 0)
      (first := jointSourceInitial) (cut := jointSourceLoanReceived)
      (word := (sourceWord jointRoundTripEvents).take 3)
      (entries := [jointEntry]) (published := [(jointEntry, jointLoanRecord)]) (pending := [])
      (channel := jointReturnChannel) (event := jointReturnFacts.effIndex)
      (witness := jointReturnWitness) (label := ⟨returnWire.frameLabel, by decide⟩)
      (data := (workerImage.decodeActorOrder? custodyChoreo).get (by decide))
      (path := ((workerImage.decodeActorOrder? custodyChoreo).get (by decide)).normalized.paths[0])
      (row := ((workerImage.decodeActorOrder? custodyChoreo).get (by decide)).rows[custodyReturnSend.event])
    · exact h2
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
  have h4 : DistributedCallExecution jointImages custodyChoreo 873 jointSourceInitial jointSourceReturnPublished
      ((sourceWord jointRoundTripEvents).take 5)
      [jointEntry, jointReturnEntry] [(jointEntry, jointLoanRecord), (jointReturnEntry, jointReturnRecord)] [] := by
    apply DistributedCallExecution.publish (left := []) (right := []) (entry := jointReturnEntry)
      (actor := 1) (record := jointReturnRecord) (after := (workerSent, jointWorkerSentMutation))
      h3 (by decide) (by decide)
    exact ActorMeasuredCommit.ordinary
      (record := ⟨jointReturnRecord, jointWorkerReceivedMutation, jointWorkerSentMutation⟩) (by decide) (by decide)
  have h5 : DistributedCallExecution jointImages custodyChoreo 873 jointSourceInitial jointSourceReturned
      (sourceWord jointRoundTripEvents)
      [jointEntry, jointReturnEntry] [(jointEntry, jointLoanRecord), (jointReturnEntry, jointReturnRecord)] [] := by
    apply DistributedCallExecution.receive (entry := jointReturnEntry) (frame := jointReturnFrame)
      (witness := jointReturnWitness) (actor := 0) (record := jointReturnReceipt)
      (channel := jointReturnChannel) (after := (ownerReturned, jointOwnerReturnedMutation))
      (next := { jointSourceReturnPublished.carriers jointReturnChannel with delivered := 1 }) h4
    · decide
    · decide
    · decide
    · rfl
    · exact a_concrete_receive_pair_has_coherent_local_witnesses (by decide)
    · exact ActorMeasuredCommit.ordinary
        (record := ⟨jointReturnReceipt, jointOwnerSentMutation, jointOwnerReturnedMutation⟩) (by decide) (by decide)
  simpa only [jointRoundTripEvents, jointEvents, sourceWord, List.filterMap_append,
    List.filterMap_cons, List.filterMap_nil, OwnedCallEvent.source?, List.append_assoc,
    List.cons_append, List.nil_append] using h5

def jointRoundTripPoints : List SourcePoint :=
  [.accepted 0, .committed ⟨0, .send⟩, .committed ⟨0, .receive⟩,
    .accepted 1, .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩]

theorem actualJointRoundTripOccurrence :
    SourceOccurrenceTrace jointImages custodyChoreo jointOccurrencePlan (sourceWord jointRoundTripEvents)
      [(jointEntry.callKey, 0), (jointReturnEntry.callKey, 1)] jointRoundTripPoints := by
  have h0 : SourceOccurrenceTrace jointImages custodyChoreo jointOccurrencePlan [] [] [] := .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := jointEntry) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.publish (key := jointEntry.callKey) (actor := 0)
    (record := jointLoanRecord) (occurrence := 0) h1 (by decide) (by decide) (by decide)
  have h3 := SourceOccurrenceTrace.receive (key := jointEntry.callKey) (actor := 1)
    (record := jointLoanReceipt) (occurrence := 0) h2 (by decide) (by decide) (by decide)
  have h4 := SourceOccurrenceTrace.accept (entry := jointReturnEntry) (occurrence := 1) h3
    (by decide) (by decide) (by decide) (by decide)
  have h5 := SourceOccurrenceTrace.publish (key := jointReturnEntry.callKey) (actor := 1)
    (record := jointReturnRecord) (occurrence := 1) h4 (by decide) (by decide) (by decide)
  have h6 := SourceOccurrenceTrace.receive (key := jointReturnEntry.callKey) (actor := 0)
    (record := jointReturnReceipt) (occurrence := 1) h5 (by decide) (by decide) (by decide)
  simpa only [jointRoundTripEvents, jointEvents, sourceWord, jointRoundTripPoints,
    List.filterMap_append, List.filterMap_cons, List.filterMap_nil, OwnedCallEvent.source?,
    List.append_assoc, List.cons_append, List.nil_append] using h6

/-- The one reconstructed source prefix and the one physical/custody history
end at the returned value. The next Read is enabled in the source and the Rust
test immediately executes that Read through the returned Endpoint. -/
theorem actualJointRoundTripContinuation :
    jointReturnedCut.custody.authorityCount ≤ 1 ∧
      jointReturnedCut.custody.usable = [returnedValue] ∧
      SourceOccurrenceExecution (occurrenceCut jointOccurrencePlan [])
        (occurrenceCut jointOccurrencePlan jointRoundTripPoints) jointRoundTripPoints ∧
      ∃ next, SourceOccurrenceStep (occurrenceCut jointOccurrencePlan jointRoundTripPoints)
        (.accepted 2) next := by
  have unfinished : (occurrenceCut jointOccurrencePlan jointRoundTripPoints).Unfinished := by
    refine ⟨.accepted 2, ?_, ?_⟩ <;> decide
  obtain ⟨affine, _, history, _, _⟩ :=
    a_joint_occurrence_refinement_has_custody_fifo_and_source_progress
      actualJointRoundTripCustody actualJointRoundTripSource actualJointRoundTripOccurrence unfinished
  exact ⟨affine, rfl, history, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

end
