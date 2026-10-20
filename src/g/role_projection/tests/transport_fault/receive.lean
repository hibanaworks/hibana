def resource : EndpointKey := ⟨0, 10⟩
def loanChannel : TransportChannel := ⟨9077, 0, loanWire.lane, loanWire.sender, loanWire.receiver⟩
def loanFrame : TransportFrame := ⟨loanChannel, 0, ⟨loanWire.frameLabel, by decide⟩⟩
def loanAdmission : SourceSendAdmission := ⟨ownerImage, ownerBefore, 0, loanSend.step.event, loanWire⟩
def loanFacts : OwnedSourceFacts := (loanAdmission.ownedFacts? faultChoreo).get (by decide)
def loanEntry : SourceAcceptance :=
  ⟨loanAdmission, loanChannel, loanFacts.effIndex, ownerBeforeMutations, loanWitness⟩
def events : List OwnedCallEvent := [
  .construct loanRequest, .accept loanEntry loanRequest loanFrame,
  .source (.publish loanEntry.callKey 0 loanSend), .poll loanRequest,
  .source (.transportFault actualFault)]
def initialCut : DistributedSourceCut := .initial fun actor =>
  if actor = 1 then (workerBefore, workerBeforeMutations) else (ownerBefore, ownerBeforeMutations)
def acceptedCut : DistributedSourceCut := initialCut.carrierAt loanChannel
  (((initialCut.carriers loanChannel).send loanFrame.frameLabel).get (by decide))
def publishedCut : DistributedSourceCut := acceptedCut.actorAt 0 (ownerAfter, ownerAfterMutations)
def finalCut : DistributedSourceCut := publishedCut.actorAt 1 (workerAfter, workerAfterMutations)

theorem actual_receive_failure_retains_the_full_accepted_prefix :
    DistributedPhysicalExecution faultImages faultChoreo 9077 initialCut finalCut (sourceWord events)
      [loanEntry] [(loanEntry, loanSend)] [] := by
  have h1 : DistributedPhysicalExecution faultImages faultChoreo 9077 initialCut acceptedCut
      [.accept loanEntry] [loanEntry] [] [loanEntry] := by
    apply DistributedPhysicalExecution.accept (sample := loanAdmission) (previousPath := 0)
      (first := initialCut) (word := []) (entries := []) (published := []) (pending := [])
      (sent := acceptedCut.carriers loanChannel) (cut := initialCut) (channel := loanChannel)
      (event := loanFacts.effIndex) (witness := loanWitness) (label := loanFrame.frameLabel)
      (data := (ownerImage.decodeActorOrder? faultChoreo).get (by decide))
      (path := ((ownerImage.decodeActorOrder? faultChoreo).get (by decide)).normalized.paths[0])
      (row := ((ownerImage.decodeActorOrder? faultChoreo).get (by decide)).rows[loanSend.step.event])
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
    (actor := 0) (record := loanSend) (after := (ownerAfter, ownerAfterMutations))
    h1 (by decide) (by decide)
    (ActorMeasuredTransition.ordinary
      (record := ⟨loanSend, ownerBeforeMutations, ownerAfterMutations⟩) (by decide) (by decide))
  exact DistributedPhysicalExecution.transportFault (sample := actualFault) h2 (by decide) (by decide)

def emptyCustody : EndpointTransfer.Observation Tag Receipt := ⟨[], [], [], []⟩
def finalCustody : Cut := ⟨[loanRequest], { emptyCustody with
  acceptedFutures := [loanRequest.payload], inFlight := [⟨loanEntry.callKey, loanFrame⟩] }⟩

theorem actual_receive_failure_does_not_create_or_reclaim_a_value :
    History faultImages resource faultChoreo loanRequest.payload events finalCustody := by
  have h1 := History.tail (History.nil (images := faultImages) (resource := resource)
    (choreo := faultChoreo) (value := loanRequest.payload))
    (Boundary.construct [] (EndpointTransfer.initial loanRequest.payload) [] [] [] loanRequest)
  have h2 := History.tail h1 (Boundary.accept _ emptyCustody [] [] [loanRequest]
    loanEntry loanRequest loanFrame loanFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold Request.matches; decide)
    (by unfold OwnedWireObservation.identifies; decide) rfl (by decide))
  have h3 := History.tail h2 (Boundary.source _ _ (.publish loanEntry.callKey 0 loanSend) trivial)
  have h4 := History.tail h3 (Boundary.poll _ _ loanRequest (by decide))
  exact History.tail h4 (Boundary.source _ _ (.transportFault actualFault) trivial)

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
def points : List SourcePoint := [.accepted 0, .committed ⟨0, .send⟩]

theorem actual_receive_failure_does_not_invent_a_source_receive :
    SourceOccurrenceTrace faultImages faultChoreo plan (sourceWord events)
      [(loanEntry.callKey, 0)] points := by
  have h0 : SourceOccurrenceTrace faultImages faultChoreo plan [] [] [] := .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := loanEntry) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.publish (key := loanEntry.callKey) (actor := 0)
    (record := loanSend) (occurrence := 0) h1 (by decide) (by decide) (by decide)
  exact SourceOccurrenceTrace.transportFault (sample := actualFault) h2

theorem actual_receive_deadline_keeps_affine_fifo_and_only_the_observers_fault :
    finalCustody.custody.authorityCount ≤ 1 ∧
      finalCustody.custody.usable = [] ∧
      finalCustody.custody.inFlight = [⟨loanEntry.callKey, loanFrame⟩] ∧
      (finalCut.actors 0).1.fault = none ∧
      (finalCut.actors 1).1.fault = some .transportClosed ∧
      ∀ channel, QualifiedDirectionHistory faultChoreo 9077
        (finalCut.carriers channel) (sourceChannelRecords [loanEntry] channel) := by
  exact ⟨(a_joint_call_history_preserves_affine_custody actual_receive_failure_does_not_create_or_reclaim_a_value).1,
    rfl, rfl, by decide, by decide,
    a_physical_execution_projects_its_complete_fifo_histories actual_receive_failure_retains_the_full_accepted_prefix⟩

theorem actual_receive_deadline_is_not_a_drain_or_eof :
    (finalCut.carriers loanChannel).peerClosed = false ∧
      (finalCut.carriers loanChannel).delivered = 0 ∧
      (finalCut.carriers loanChannel).frames = [loanFrame] ∧
      workerAfter.viewIssued 10 = false ∧
      ownerAfter.viewIssued 10 = false ∧
      SourcePoint.committed ⟨0, .receive⟩ ∉ points := by decide
