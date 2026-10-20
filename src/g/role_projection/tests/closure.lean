open Hibana Hibana.EndpointDelegation Hibana.OwnedCallHistory

def closureImages (actor : Nat) : RustDescriptorImage :=
  if actor = 1 then workerImage else ownerImage
def closureResource : EndpointKey := ⟨0, 10⟩
def returnChannel : TransportChannel :=
  { session := returnWire.session, generation := 0, lane := returnWire.lane, sender := 1, receiver := 0 }
def loanFrame : TransportFrame :=
  ⟨loanChannel, 0, ⟨loanWire.frameLabel, by decide⟩⟩
def returnFrame : TransportFrame :=
  ⟨returnChannel, 0, ⟨returnWire.frameLabel, by decide⟩⟩
def loanAdmission : SourceSendAdmission :=
  ⟨ownerImage, ownerPending, 0, loanSend.step.event, loanWire⟩
def returnAdmission : SourceSendAdmission :=
  ⟨workerImage, workerPending, 0, returnSend.step.event, returnWire⟩
def loanFacts : OwnedSourceFacts := (loanAdmission.ownedFacts? closureChoreo).get (by decide)
def returnFacts : OwnedSourceFacts := (returnAdmission.ownedFacts? closureChoreo).get (by decide)
def loanEntry : SourceAcceptance :=
  ⟨loanAdmission, loanChannel, loanFacts.effIndex, ownerPendingMutations, loanRequestWitness⟩
def returnEntry : SourceAcceptance :=
  ⟨returnAdmission, returnChannel, returnFacts.effIndex, workerPendingMutations, returnRequestWitness⟩
def loanReceiptFacts : OwnedCommitFacts :=
  (loanRecv.step.ownedFacts? workerImage closureChoreo).get (by decide)
def returnReceiptFacts : OwnedCommitFacts :=
  (returnRecv.step.ownedFacts? ownerImage closureChoreo).get (by decide)
def receivedValue : Tag := (1, loanReceiptFacts.afterView.header)
def returnedValue : Tag := (0, returnReceiptFacts.afterView.header)

/-- This is the recorded order, including both Pending polls, successful polls,
completed-future destruction, physical close, and the later EOF observation. -/
def closureEvents : List OwnedCallEvent := [
  .construct loanRequest, .poll loanRequest, .accept loanEntry loanRequest loanFrame,
  .source (.publish loanEntry.callKey 0 loanSend), .poll loanRequest, .retire loanRequest,
  .source (.close loanChannel),
  .receive loanEntry.callKey 1 loanRecv loanFrame receivedValue,
  .construct returnRequest, .poll returnRequest, .source (.quiet 1),
  .accept returnEntry returnRequest returnFrame,
  .source (.publish returnEntry.callKey 1 returnSend), .poll returnRequest, .retire returnRequest,
  .receive returnEntry.callKey 0 returnRecv returnFrame returnedValue,
  .source (.observeClosure actualEof)]
def closureInitial : DistributedSourceCut := .initial fun actor =>
  if actor = 1 then (workerBefore, workerBeforeMutations) else (ownerPending, ownerPendingMutations)
def closureAccepted : DistributedSourceCut := closureInitial.carrierAt loanChannel
  (((closureInitial.carriers loanChannel).send loanFrame.frameLabel).get (by decide))
def closurePublished : DistributedSourceCut := closureAccepted.actorAt 0 (ownerSent, ownerSentMutations)
def closureClosed : DistributedSourceCut := closurePublished.carrierAt loanChannel
  (closurePublished.carriers loanChannel).closePeer
def closureReceived : DistributedSourceCut :=
  (closureClosed.actorAt 1 (workerReceived, workerReceivedMutations)).carrierAt loanChannel
    { closureClosed.carriers loanChannel with delivered := 1 }
def closurePrepared : DistributedSourceCut := closureReceived.actorAt 1 (workerPending, workerPendingMutations)
def closureReturnAccepted : DistributedSourceCut := closurePrepared.carrierAt returnChannel
  (((closurePrepared.carriers returnChannel).send returnFrame.frameLabel).get (by decide))
def closureReturnPublished : DistributedSourceCut :=
  closureReturnAccepted.actorAt 1 (workerSent, workerSentMutations)
def closureReturned : DistributedSourceCut :=
  (closureReturnPublished.actorAt 0 (ownerReturned, ownerReturnedMutations)).carrierAt returnChannel
    { closureReturnPublished.carriers returnChannel with delivered := 1 }
def closureFinal : DistributedSourceCut :=
  closureReturned.actorAt 1 (workerAfterEof, workerBeforeEofMutations)

theorem actualClosurePhysicalHistory :
    DistributedPhysicalExecution closureImages closureChoreo 9060 closureInitial closureFinal
      (sourceWord closureEvents) [loanEntry, returnEntry]
      [(loanEntry, loanSend), (returnEntry, returnSend)] [] := by
  have h1 : DistributedPhysicalExecution closureImages closureChoreo 9060
      closureInitial closureAccepted [.accept loanEntry] [loanEntry] [] [loanEntry] := by
    apply DistributedPhysicalExecution.accept (sample := loanAdmission) (previousPath := 0)
      (first := closureInitial) (word := []) (entries := []) (published := []) (pending := [])
      (sent := closureAccepted.carriers loanChannel)
      (cut := closureInitial) (channel := loanChannel) (event := loanFacts.effIndex)
      (witness := loanRequestWitness) (label := loanFrame.frameLabel)
      (data := (ownerImage.decodeActorOrder? closureChoreo).get (by decide))
      (path := ((ownerImage.decodeActorOrder? closureChoreo).get (by decide)).normalized.paths[0])
      (row := ((ownerImage.decodeActorOrder? closureChoreo).get (by decide)).rows[loanSend.step.event])
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
  have h2 := DistributedPhysicalExecution.publish (left := []) (right := []) (entry := loanEntry)
    (actor := 0) (record := loanSend) (after := (ownerSent, ownerSentMutations))
    h1 (by decide) (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨loanSend, ownerPendingMutations, ownerSentMutations⟩) (by decide) (by decide))
  have h3 := DistributedPhysicalExecution.close (channel := loanChannel) h2
  have h4 := DistributedPhysicalExecution.receive (entry := loanEntry) (frame := loanFrame)
    (witness := loanRequestWitness) (actor := 1) (record := loanRecv)
    (channel := loanChannel) (after := (workerReceived, workerReceivedMutations))
    (next := { closureClosed.carriers loanChannel with delivered := 1 })
    h3 (by decide) (by decide) (by decide) rfl (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨loanRecv, workerBeforeMutations, workerReceivedMutations⟩) (by decide) (by decide))
  have h5 := DistributedPhysicalExecution.quiet (actor := 1) (after := workerPending)
    h4 (by decide) (by decide)
  have h6 : DistributedPhysicalExecution closureImages closureChoreo 9060 closureInitial
      closureReturnAccepted ((sourceWord closureEvents).take 6) [loanEntry, returnEntry]
      [(loanEntry, loanSend)] [returnEntry] := by
    apply DistributedPhysicalExecution.accept (sample := returnAdmission) (previousPath := 0)
      (first := closureInitial) (word := (sourceWord closureEvents).take 5)
      (entries := [loanEntry]) (published := [(loanEntry, loanSend)]) (pending := [])
      (sent := closureReturnAccepted.carriers returnChannel)
      (cut := closurePrepared) (channel := returnChannel) (event := returnFacts.effIndex)
      (witness := returnRequestWitness) (label := returnFrame.frameLabel)
      (data := (workerImage.decodeActorOrder? closureChoreo).get (by decide))
      (path := ((workerImage.decodeActorOrder? closureChoreo).get (by decide)).normalized.paths[0])
      (row := ((workerImage.decodeActorOrder? closureChoreo).get (by decide)).rows[returnSend.step.event])
    · exact h5
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
  have h7 := DistributedPhysicalExecution.publish (left := []) (right := []) (entry := returnEntry)
    (actor := 1) (record := returnSend) (after := (workerSent, workerSentMutations))
    h6 (by decide) (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨returnSend, workerPendingMutations, workerSentMutations⟩) (by decide) (by decide))
  have h8 := DistributedPhysicalExecution.receive (entry := returnEntry) (frame := returnFrame)
    (witness := returnRequestWitness) (actor := 0) (record := returnRecv)
    (channel := returnChannel) (after := (ownerReturned, ownerReturnedMutations))
    (next := { closureReturnPublished.carriers returnChannel with delivered := 1 })
    h7 (by decide) (by decide) (by decide) rfl (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨returnRecv, ownerSentMutations, ownerReturnedMutations⟩) (by decide) (by decide))
  exact DistributedPhysicalExecution.observeClosure (sample := actualEof)
    h8 actualClosureCheck (by decide) (by decide) (by decide)

def emptyCustody : EndpointTransfer.Observation Tag Receipt := ⟨[], [], [], []⟩
def closureCustody : Cut := ⟨[], { emptyCustody with usable := [returnedValue] }⟩

theorem actualClosureCustodyHistory :
    History closureImages closureResource closureChoreo loanRequest.payload closureEvents closureCustody := by
  have h1 := History.tail (History.nil (images := closureImages) (resource := closureResource)
    (choreo := closureChoreo) (value := loanRequest.payload))
    (Boundary.construct [] (EndpointTransfer.initial loanRequest.payload) [] [] [] loanRequest)
  have h2 := History.tail h1 (Boundary.poll _ _ loanRequest (by decide))
  have h3 := History.tail h2 (Boundary.accept _ emptyCustody [] [] [loanRequest]
    loanEntry loanRequest loanFrame loanFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold Request.matches; decide)
    (by unfold OwnedWireObservation.identifies; decide) rfl (by decide))
  have h4 := History.tail h3 (Boundary.source _ _ (.publish loanEntry.callKey 0 loanSend) trivial)
  have h5 := History.tail h4 (Boundary.poll _ _ loanRequest (by decide))
  have h6 := History.tail h5 (Boundary.retire _
    { emptyCustody with inFlight := [⟨loanEntry.callKey, loanFrame⟩] } [] [] [] [] loanRequest)
  have h7 := History.tail h6 (Boundary.source _ _ (.close loanChannel) trivial)
  have h8 := History.tail h7 (Boundary.receive _ emptyCustody [] []
    loanEntry.callKey 1 loanRecv loanFrame loanReceiptFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold OwnedWireObservation.identifies; decide))
  have same : receivedValue = returnRequest.payload := by decide
  change History closureImages closureResource closureChoreo loanRequest.payload _
    ⟨[], { emptyCustody with usable := [receivedValue] }⟩ at h8
  rw [same] at h8
  have h9 := History.tail h8 (Boundary.construct _ (EndpointTransfer.initial returnRequest.payload)
    [] [] [] returnRequest)
  have h10 := History.tail h9 (Boundary.poll _ _ returnRequest (by decide))
  have h11 := History.tail h10 (Boundary.source _ _ (.quiet 1) trivial)
  have h12 := History.tail h11 (Boundary.accept _ emptyCustody [] [] [returnRequest]
    returnEntry returnRequest returnFrame returnFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold Request.matches; decide)
    (by unfold OwnedWireObservation.identifies; decide) rfl (by decide))
  have h13 := History.tail h12 (Boundary.source _ _ (.publish returnEntry.callKey 1 returnSend) trivial)
  have h14 := History.tail h13 (Boundary.poll _ _ returnRequest (by decide))
  have h15 := History.tail h14 (Boundary.retire _
    { emptyCustody with inFlight := [⟨returnEntry.callKey, returnFrame⟩] } [] [] [] [] returnRequest)
  have h16 := History.tail h15 (Boundary.receive _ emptyCustody [] []
    returnEntry.callKey 0 returnRecv returnFrame returnReceiptFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold OwnedWireObservation.identifies; decide))
  exact History.tail h16 (Boundary.source _ _ (.observeClosure actualEof) trivial)

def closureSource : Source := (closureChoreo.ownershipSource?).get (by decide)
def closureOriginal : Normalization := (normalize closureSource).get (by decide)
def closureNormalized : Normalization := (normalize closureSource.oneCopy).get (by decide)
def closurePlan : SourceOccurrenceCut closureSource := {
  original := closureOriginal
  originalAccepted := (Option.some_get _).symm
  expanded := closureSource.oneCopy
  unfolding := SourceUnfolding.oneCopy _
  normalized := closureNormalized
  accepted := (Option.some_get _).symm
  sameIssuance := by decide
  path := closureNormalized.paths[0]
  selected := by decide
  observed := fun _ => false
}
def closurePoints : List SourcePoint :=
  [.accepted 0, .committed ⟨0, .send⟩, .committed ⟨0, .receive⟩,
   .accepted 1, .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩]

theorem actualClosureSourceHistory : SourceOccurrenceTrace closureImages closureChoreo closurePlan
    (sourceWord closureEvents) [(loanEntry.callKey, 0), (returnEntry.callKey, 1)] closurePoints := by
  have h0 : SourceOccurrenceTrace closureImages closureChoreo closurePlan [] [] [] := .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := loanEntry) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.publish (key := loanEntry.callKey) (actor := 0)
    (record := loanSend) (occurrence := 0) h1 (by decide) (by decide) (by decide)
  have h3 := SourceOccurrenceTrace.close (channel := loanChannel) h2
  have h4 := SourceOccurrenceTrace.receive (key := loanEntry.callKey) (actor := 1)
    (record := loanRecv) (occurrence := 0) h3 (by decide) (by decide) (by decide)
  have h5 := SourceOccurrenceTrace.quiet (actor := 1) h4
  have h6 := SourceOccurrenceTrace.accept (entry := returnEntry) (occurrence := 1) h5
    (by decide) (by decide) (by decide) (by decide)
  have h7 := SourceOccurrenceTrace.publish (key := returnEntry.callKey) (actor := 1)
    (record := returnSend) (occurrence := 1) h6 (by decide) (by decide) (by decide)
  have h8 := SourceOccurrenceTrace.receive (key := returnEntry.callKey) (actor := 0)
    (record := returnRecv) (occurrence := 1) h7 (by decide) (by decide) (by decide)
  exact SourceOccurrenceTrace.observeClosure (sample := actualEof) h8

/-- One word gives custody, FIFO history and source progress. The next enabled
Read belongs to the live owner, not the worker that observed EOF; Rust actually
executes that Read after capturing this final cut. -/
theorem actualClosureKeepsReturnedAuthorityAndTheLiveContinuation :
    closureCustody.custody.authorityCount ≤ 1 ∧
      closureCustody.custody.usable = [returnedValue] ∧
      (closureFinal.actors 1).1.fault = some .transportClosed ∧
      (closureFinal.actors 0).1.fault = none ∧
      ∃ next, SourceOccurrenceStep (occurrenceCut closurePlan closurePoints) (.accepted 2) next := by
  have unfinished : (occurrenceCut closurePlan closurePoints).Unfinished := by
    refine ⟨.accepted 2, ?_, ?_⟩ <;> decide
  have combined := a_joint_physical_occurrence_refinement_has_custody_fifo_and_source_progress
    actualClosureCustodyHistory actualClosurePhysicalHistory actualClosureSourceHistory unfinished
  refine ⟨combined.1, rfl, ?_, ?_, ?_⟩
  · decide
  · decide
  · exact a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)

theorem actualClosureCannotBeObservedBeforeTheAcceptedLoanDrains :
    (closureClosed.carriers loanChannel).pollReceive ≠ .peerClosed := by decide

theorem actualClosureRejectsWrongDirectionAndHiddenProgress :
    ({ actualEof with channel := returnChannel }.check workerImage closureChoreo 9060 = false) ∧
    ({ actualEof with after := { workerAfterEof with counter := workerAfterEof.counter + 1 } }.check
      workerImage closureChoreo 9060 = false) ∧
    ({ actualEof with after := { workerAfterEof with fault := none } }.check
      workerImage closureChoreo 9060 = false) := by decide
