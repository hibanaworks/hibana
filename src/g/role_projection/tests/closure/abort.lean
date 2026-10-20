open Hibana Hibana.EndpointDelegation Hibana.OwnedCallHistory

def abortImages (actor : Nat) : RustDescriptorImage :=
  if actor = 1 then workerImage else ownerImage
def abortResource : EndpointKey := ⟨0, 10⟩
def loanChannel : TransportChannel :=
  ⟨9061, 0, loanWire.lane, loanWire.sender, loanWire.receiver⟩
def loanFrame : TransportFrame := ⟨loanChannel, 0, ⟨loanWire.frameLabel, by decide⟩⟩
def loanReceipt : Receipt := ⟨(loanChannel, loanWitness), loanFrame⟩
def loanAdmission : SourceSendAdmission :=
  ⟨ownerImage, ownerPending, 0, loanSend.step.event, loanWire⟩
def loanFacts : OwnedSourceFacts := (loanAdmission.ownedFacts? abortChoreo).get (by decide)
def loanEntry : SourceAcceptance :=
  ⟨loanAdmission, loanChannel, loanFacts.effIndex, ownerPendingMutations, loanWitness⟩
def actualAbortEof : ActorClosureObservation :=
  { channel := loanChannel, event := 0, before := workerBefore, after := workerFaulted }
def abortEvents : List OwnedCallEvent := [
  .construct loanRequest, .poll loanRequest, .accept loanEntry loanRequest loanFrame,
  .source (.publish loanEntry.callKey 0 loanSend), .poll loanRequest, .retire loanRequest,
  .fence loanReceipt, .source (.observeClosure actualAbortEof)]

def abortInitial : DistributedSourceCut := .initial fun actor =>
  if actor = 1 then (workerBefore, workerBeforeMutations) else (ownerPending, ownerPendingMutations)
def abortAccepted : DistributedSourceCut := abortInitial.carrierAt loanChannel
  (((abortInitial.carriers loanChannel).send loanFrame.frameLabel).get (by decide))
def abortPublished : DistributedSourceCut := abortAccepted.actorAt 0 (ownerSent, ownerSentMutations)
def abortIsolated : DistributedSourceCut := abortPublished.carrierAt loanChannel
  (abortPublished.carriers loanChannel).abortPeer
def abortFinal : DistributedSourceCut := abortIsolated.actorAt 1 (workerFaulted, workerBeforeMutations)

theorem actualAbortPhysicalHistory : DistributedPhysicalExecution abortImages abortChoreo 9061
    abortInitial abortFinal (sourceWord abortEvents) [loanEntry] [(loanEntry, loanSend)] [] := by
  have h1 : DistributedPhysicalExecution abortImages abortChoreo 9061
      abortInitial abortAccepted [.accept loanEntry] [loanEntry] [] [loanEntry] := by
    apply DistributedPhysicalExecution.accept (sample := loanAdmission) (previousPath := 0)
      (first := abortInitial) (word := []) (entries := []) (published := []) (pending := [])
      (sent := abortAccepted.carriers loanChannel)
      (cut := abortInitial) (channel := loanChannel) (event := loanFacts.effIndex)
      (witness := loanWitness) (label := loanFrame.frameLabel)
      (data := (ownerImage.decodeActorOrder? abortChoreo).get (by decide))
      (path := ((ownerImage.decodeActorOrder? abortChoreo).get (by decide)).normalized.paths[0])
      (row := ((ownerImage.decodeActorOrder? abortChoreo).get (by decide)).rows[loanSend.step.event])
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
  have h3 := DistributedPhysicalExecution.abort (channel := loanChannel) h2
  exact DistributedPhysicalExecution.observeClosure (sample := actualAbortEof)
    h3 (by decide) (by decide) (by decide) (by decide)

def emptyCustody : EndpointTransfer.Observation Tag Receipt := ⟨[], [], [], []⟩
def beforeFence : Cut := ⟨[], { emptyCustody with inFlight := [loanReceipt] }⟩
def abortCustody : Cut := ⟨[], emptyCustody⟩

theorem actualAbortCustodyHistory :
    History abortImages abortResource abortChoreo loanRequest.payload abortEvents abortCustody := by
  have h1 := History.tail (History.nil (images := abortImages) (resource := abortResource)
    (choreo := abortChoreo) (value := loanRequest.payload))
    (Boundary.construct [] (EndpointTransfer.initial loanRequest.payload) [] [] [] loanRequest)
  have h2 := History.tail h1 (Boundary.poll _ _ loanRequest (by decide))
  have h3 := History.tail h2 (Boundary.accept _ emptyCustody [] [] [loanRequest]
    loanEntry loanRequest loanFrame loanFacts (Option.some_get _).symm
    (by decide) (by decide) (by decide) (by unfold Request.matches; decide)
    (by unfold OwnedWireObservation.identifies; decide) rfl (by decide))
  have h4 := History.tail h3 (Boundary.source _ _ (.publish loanEntry.callKey 0 loanSend) trivial)
  have h5 := History.tail h4 (Boundary.poll _ _ loanRequest (by decide))
  have h6 := History.tail h5 (Boundary.retire _
    { emptyCustody with inFlight := [loanReceipt] } [] [] [] [] loanRequest)
  have h7 := History.tail h6 (Boundary.fence _ emptyCustody [] [] [] loanReceipt)
  exact History.tail h7 (Boundary.source _ _ (.observeClosure actualAbortEof) trivial)

def abortSource : Source := (abortChoreo.ownershipSource?).get (by decide)
def abortOriginal : Normalization := (normalize abortSource).get (by decide)
def abortNormalized : Normalization := (normalize abortSource.oneCopy).get (by decide)
def abortPlan : SourceOccurrenceCut abortSource := {
  original := abortOriginal
  originalAccepted := (Option.some_get _).symm
  expanded := abortSource.oneCopy
  unfolding := SourceUnfolding.oneCopy _
  normalized := abortNormalized
  accepted := (Option.some_get _).symm
  sameIssuance := by decide
  path := abortNormalized.paths[0]
  selected := by decide
  observed := fun _ => false
}
def abortPoints : List SourcePoint := [.accepted 0, .committed ⟨0, .send⟩]

theorem actualAbortSourceHistory : SourceOccurrenceTrace abortImages abortChoreo abortPlan
    (sourceWord abortEvents) [(loanEntry.callKey, 0)] abortPoints := by
  have h0 : SourceOccurrenceTrace abortImages abortChoreo abortPlan [] [] [] := .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := loanEntry) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.publish (key := loanEntry.callKey) (actor := 0)
    (record := loanSend) (occurrence := 0) h1 (by decide) (by decide) (by decide)
  have h3 := SourceOccurrenceTrace.abort (channel := loanChannel) h2
  exact SourceOccurrenceTrace.observeClosure (sample := actualAbortEof) h3

/-- The pre-abort accepted source prefix survives as retirement evidence. There
is no receive in the source word and no callable receiver value in custody. -/
theorem actualAbortRetainsThePrefixWithoutInventingDeliveryOrAuthority :
    History abortImages abortResource abortChoreo loanRequest.payload abortEvents abortCustody ∧
    SourceOccurrenceExecution (occurrenceCut abortPlan []) (occurrenceCut abortPlan abortPoints) abortPoints ∧
    QualifiedDirectionHistory abortChoreo 9061 (abortFinal.carriers loanChannel)
      (sourceChannelRecords [loanEntry] loanChannel) ∧
    abortCustody.custody.authorityCount = 0 ∧ abortCustody.custody.inFlight = [] ∧
    (abortFinal.actors 1).1.fault = some .transportClosed ∧
    (abortFinal.actors 0).1.fault = none ∧
    abortPoints.contains (.committed ⟨0, .receive⟩) = false := by
  have source := a_source_occurrence_trace_is_its_reconstructed_execution actualAbortSourceHistory
  have histories := a_physical_execution_projects_its_complete_fifo_histories actualAbortPhysicalHistory
  exact ⟨actualAbortCustodyHistory, source, histories loanChannel,
    by decide, rfl, by decide, by decide, by decide⟩

theorem an_untracked_abort_cannot_hide_this_in_flight_owned_frame :
    ¬ Untracked abortResource abortChoreo (abortEvents.take 6) beforeFence (.abort loanChannel) := by
  intro hidden
  exact hidden loanReceipt (by simp [beforeFence]) rfl
