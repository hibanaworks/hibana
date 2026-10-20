def resource : EndpointKey := ⟨0, 10⟩
def loanChannel : TransportChannel := ⟨9077, 0, loanWire.lane, loanWire.sender, loanWire.receiver⟩
def readChannel : TransportChannel := ⟨9077, 0, readWire.lane, readWire.sender, readWire.receiver⟩
def loanFrame : TransportFrame := ⟨loanChannel, 0, ⟨loanWire.frameLabel, by decide⟩⟩
def readFrame : TransportFrame := ⟨readChannel, 0, ⟨readWire.frameLabel, by decide⟩⟩
def loanEvent : Nat := (resolveUnique? ((List.range ownerImage.eventCount).filter fun row =>
  (sourceOperationIdentity? ownerImage faultChoreo 0 row).any fun (_, binding, side) =>
    binding.message.label == loanRequest.label && side == .send)).get (by decide)
def loanAdmission : SourceSendAdmission := ⟨ownerImage, ownerBefore, 0, loanEvent, loanWire⟩
def loanFacts : OwnedSourceFacts := (loanAdmission.ownedFacts? faultChoreo).get (by decide)
def loanEntry : SourceAcceptance :=
  ⟨loanAdmission, loanChannel, loanFacts.effIndex, ownerBeforeMutations, loanWitness⟩
def readAdmission : SourceSendAdmission := ⟨workerImage, workerReceived, 0, readSend.step.event, readWire⟩
def readEntry : SourceAcceptance := ⟨readAdmission, readChannel, 1, workerReceivedMutations, readWitness⟩
def receiptFacts : OwnedCommitFacts := (loanRecv.step.ownedFacts? workerImage faultChoreo).get (by decide)
def receivedValue : Tag := (1, receiptFacts.afterView.header)
def events : List OwnedCallEvent := [
  .construct loanRequest, .accept loanEntry loanRequest loanFrame,
  .source (.transportFault actualFault), .poll loanRequest,
  .receive loanEntry.callKey 1 loanRecv loanFrame receivedValue,
  .source (.accept readEntry), .source (.publish readEntry.callKey 1 readSend),
  .source (.receive readEntry.callKey 1 readRecv)]

def initialCut : DistributedSourceCut := .initial fun actor =>
  if actor = 1 then (workerBefore, workerBeforeMutations) else (ownerBefore, ownerBeforeMutations)
def acceptedCut : DistributedSourceCut := initialCut.carrierAt loanChannel
  (((initialCut.carriers loanChannel).send loanFrame.frameLabel).get (by decide))
def faultedCut : DistributedSourceCut := acceptedCut.actorAt 0 (ownerAfter, ownerAfterMutations)
def receivedCut : DistributedSourceCut :=
  (faultedCut.actorAt 1 (workerReceived, workerReceivedMutations)).carrierAt loanChannel
    { faultedCut.carriers loanChannel with delivered := 1 }
def readAcceptedCut : DistributedSourceCut := receivedCut.carrierAt readChannel
  (((receivedCut.carriers readChannel).send readFrame.frameLabel).get (by decide))
def readPublishedCut : DistributedSourceCut := readAcceptedCut.actorAt 1 (workerSent, workerSentMutations)
def finalCut : DistributedSourceCut :=
  (readPublishedCut.actorAt 1 (workerDone, workerDoneMutations)).carrierAt readChannel
    { readPublishedCut.carriers readChannel with delivered := 1 }

theorem actual_sender_failure_and_remote_use_share_one_physical_word :
    DistributedPhysicalExecution faultImages faultChoreo 9077 initialCut finalCut (sourceWord events)
      [loanEntry, readEntry] [(readEntry, readSend)] [loanEntry] := by
  have h1 : DistributedPhysicalExecution faultImages faultChoreo 9077 initialCut acceptedCut
      [.accept loanEntry] [loanEntry] [] [loanEntry] := by
    apply DistributedPhysicalExecution.accept (sample := loanAdmission) (previousPath := 0)
      (first := initialCut) (word := []) (entries := []) (published := []) (pending := [])
      (sent := acceptedCut.carriers loanChannel) (cut := initialCut) (channel := loanChannel)
      (event := loanFacts.effIndex) (witness := loanWitness) (label := loanFrame.frameLabel)
      (data := (ownerImage.decodeActorOrder? faultChoreo).get (by decide))
      (path := ((ownerImage.decodeActorOrder? faultChoreo).get (by decide)).normalized.paths[0])
      (row := ((ownerImage.decodeActorOrder? faultChoreo).get (by decide)).rows[loanEvent])
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
  have h2 := DistributedPhysicalExecution.transportFault (sample := actualFault) h1 (by decide) (by decide)
  have h3 := DistributedPhysicalExecution.receive (entry := loanEntry) (frame := loanFrame)
    (witness := loanWitness) (actor := 1) (record := loanRecv)
    (channel := loanChannel) (after := (workerReceived, workerReceivedMutations))
    (next := { faultedCut.carriers loanChannel with delivered := 1 })
    h2 (by decide) (by decide) (by decide) rfl (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨loanRecv, workerBeforeMutations, workerReceivedMutations⟩) (by decide) (by decide))
  have h4 : DistributedPhysicalExecution faultImages faultChoreo 9077 initialCut readAcceptedCut
      ((sourceWord events).take 4) [loanEntry, readEntry] [] [loanEntry, readEntry] := by
    apply DistributedPhysicalExecution.accept (sample := readAdmission) (previousPath := 0)
      (first := initialCut) (word := (sourceWord events).take 3)
      (entries := [loanEntry]) (published := []) (pending := [loanEntry])
      (sent := readAcceptedCut.carriers readChannel) (cut := receivedCut) (channel := readChannel)
      (event := 1) (witness := readWitness) (label := readFrame.frameLabel)
      (data := (workerImage.decodeActorOrder? faultChoreo).get (by decide))
      (path := ((workerImage.decodeActorOrder? faultChoreo).get (by decide)).normalized.paths[0])
      (row := ((workerImage.decodeActorOrder? faultChoreo).get (by decide)).rows[readSend.step.event])
    · exact h3
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
  have h5 := DistributedPhysicalExecution.publish (left := [loanEntry]) (right := []) (entry := readEntry)
    (actor := 1) (record := readSend) (after := (workerSent, workerSentMutations))
    h4 (by decide) (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨readSend, workerReceivedMutations, workerSentMutations⟩) (by decide) (by decide))
  exact DistributedPhysicalExecution.receive (entry := readEntry) (frame := readFrame)
    (witness := readWitness) (actor := 1) (record := readRecv)
    (channel := readChannel) (after := (workerDone, workerDoneMutations))
    (next := { readPublishedCut.carriers readChannel with delivered := 1 })
    h5 (by decide) (by decide) (by decide) rfl (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨readRecv, workerSentMutations, workerDoneMutations⟩) (by decide) (by decide))

def emptyCustody : EndpointTransfer.Observation Tag Receipt := ⟨[], [], [], []⟩
def finalCustody : Cut :=
  ⟨[loanRequest], { emptyCustody with usable := [receivedValue], acceptedFutures := [loanRequest.payload] }⟩

theorem actual_sender_failure_keeps_the_same_affine_transfer :
    History faultImages resource faultChoreo loanRequest.payload events finalCustody := by
  have h1 := History.tail (History.nil (images := faultImages) (resource := resource)
    (choreo := faultChoreo) (value := loanRequest.payload))
    (Boundary.construct [] (EndpointTransfer.initial loanRequest.payload) [] [] [] loanRequest)
  have h2 := History.tail h1 (Boundary.accept _ emptyCustody [] [] [loanRequest]
    loanEntry loanRequest loanFrame loanFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold Request.matches; decide)
    (by unfold OwnedWireObservation.identifies; decide) rfl (by decide))
  have h3 := History.tail h2 (Boundary.source _ _ (.transportFault actualFault) trivial)
  have h4 := History.tail h3 (Boundary.poll _ _ loanRequest (by decide))
  have h5 := History.tail h4 (Boundary.receive _
    { emptyCustody with acceptedFutures := [loanRequest.payload] } [] [loanRequest]
    loanEntry.callKey 1 loanRecv loanFrame receiptFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold OwnedWireObservation.identifies; decide))
  have h6 := History.tail h5 (Boundary.source _ _ (.accept readEntry)
    (by change tracks resource faultChoreo readEntry = false; decide))
  have h7 := History.tail h6 (Boundary.source _ _ (.publish readEntry.callKey 1 readSend) trivial)
  exact History.tail h7 (Boundary.source _ _ (.receive readEntry.callKey 1 readRecv)
    ⟨readEntry, by decide, rfl, by decide⟩)

def source : Source := (faultChoreo.ownershipSource?).get (by decide)
def original : Normalization := (normalize source).get (by decide)
def expanded : Normalization := (normalize source.oneCopy).get (by decide)
def plan : SourceOccurrenceCut source := {
  original, originalAccepted := (Option.some_get _).symm
  expanded := source.oneCopy, unfolding := SourceUnfolding.oneCopy _
  normalized := expanded, accepted := (Option.some_get _).symm
  sameIssuance := by decide
  path := expanded.paths[0], selected := by decide
  observed := fun _ => false
}
def points : List SourcePoint :=
  [.accepted 0, .committed ⟨0, .receive⟩, .accepted 1,
    .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩]

theorem actual_sender_failure_does_not_invent_publication_or_rollback :
    SourceOccurrenceTrace faultImages faultChoreo plan (sourceWord events)
      [(loanEntry.callKey, 0), (readEntry.callKey, 1)] points := by
  have h0 : SourceOccurrenceTrace faultImages faultChoreo plan [] [] [] := .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := loanEntry) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.transportFault (sample := actualFault) h1
  have h3 := SourceOccurrenceTrace.receive (key := loanEntry.callKey) (actor := 1)
    (record := loanRecv) (occurrence := 0) h2 (by decide) (by decide) (by decide)
  have h4 := SourceOccurrenceTrace.accept (entry := readEntry) (occurrence := 1) h3
    (by decide) (by decide) (by decide) (by decide)
  have h5 := SourceOccurrenceTrace.publish (key := readEntry.callKey) (actor := 1)
    (record := readSend) (occurrence := 1) h4 (by decide) (by decide) (by decide)
  exact SourceOccurrenceTrace.receive (key := readEntry.callKey) (actor := 1)
    (record := readRecv) (occurrence := 1) h5 (by decide) (by decide) (by decide)

theorem actual_deadline_preserves_the_remote_right_and_all_fifo_history :
    finalCustody.custody.authorityCount ≤ 1 ∧
      finalCustody.custody.usable = [receivedValue] ∧
      (finalCut.actors 0).1.fault = some .transportClosed ∧
      (finalCut.actors 1).1.fault = none ∧
      ∀ channel, QualifiedDirectionHistory faultChoreo 9077
        (finalCut.carriers channel) (sourceChannelRecords [loanEntry, readEntry] channel) := by
  exact ⟨(a_joint_call_history_preserves_affine_custody actual_sender_failure_keeps_the_same_affine_transfer).1,
    rfl, by decide, by decide,
    a_physical_execution_projects_its_complete_fifo_histories actual_sender_failure_and_remote_use_share_one_physical_word⟩

theorem actual_deadline_is_not_eof_or_a_permitted_local_retry :
    (finalCut.carriers loanChannel).peerClosed = false ∧
      (finalCut.carriers readChannel).peerClosed = false ∧
      (observedGeneration actualFault.after.fault).publicationPermitted = false ∧
      .committed ⟨0, .send⟩ ∉ points := by decide
