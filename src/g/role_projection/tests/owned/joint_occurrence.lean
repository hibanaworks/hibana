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

/-- The earlier worker receipt is no longer local at the final cut. The
general same-word result recovers its actual subsequent release, rather than
assuming a Rust value still exists because the source receive is complete. -/
theorem actualJointLoanReceiptHasItsConcreteLaterRelease :
    ∃ frame earlier later event before after,
      jointRoundTripEvents = earlier ++ event :: later ∧
      OwnedCallEvent.receive jointEntry.callKey 1 jointLoanReceipt frame receivedValue ∈ earlier ∧
      History jointImages jointResource custodyChoreo initialValue earlier before ∧
      Boundary jointImages jointResource custodyChoreo earlier event before after ∧
      TransferValueLocated receivedValue before.custody ∧ event.releasedValue? = some receivedValue := by
  obtain ⟨frame, facts, _, factsAt, _, _, _, _, outcome⟩ :=
    an_owned_occurrence_receipt_has_its_actual_value_or_subsequent_release
      (occurrence := 0) (key := jointEntry.callKey) (actor := 1) (record := jointLoanReceipt)
      (binding := jointOccurrencePlan.path.bindings[0].2)
      actualJointRoundTripCustody
      (a_selected_visit_execution_has_its_same_physical_execution actualJointRoundTripSource)
      actualJointRoundTripOccurrence (by decide) (by decide)
      (by simp [jointRoundTripEvents, jointEvents, sourceWord, OwnedCallEvent.source?]) (by decide)
  have known : jointLoanReceipt.step.ownedFacts? (jointImages 1) custodyChoreo =
      some jointLoanReceiptFacts := (Option.some_get _).symm
  have same := Option.some.inj (factsAt.symm.trans known)
  subst facts
  have tag : (1, jointLoanReceiptFacts.afterView.header) = receivedValue := by decide
  rw [tag] at outcome
  rcases outcome with retained | released
  · have absent : ¬ TransferValueLocated receivedValue jointReturnedCut.custody := by
      unfold TransferValueLocated
      decide
    exact False.elim (absent retained)
  · obtain ⟨earlier, later, event, before, after, wordAt, receiptAt, history, boundary, located, releaseAt⟩ := released
    exact ⟨frame, earlier, later, event, before, after,
      wordAt, receiptAt, history, boundary, located, releaseAt⟩

/-- The latest owner receipt derives the exact retained value through the
same-word theorem. Completed sender-witness drops cannot release this tag;
the final usable value is not supplied as an availability premise. -/
theorem actualJointReturnedReceiptDerivesItsRetainedValue :
    ∃ frame,
      OwnedCallEvent.receive jointReturnEntry.callKey 0 jointReturnReceipt frame returnedValue ∈ jointRoundTripEvents ∧
      TransferValueLocated returnedValue jointReturnedCut.custody := by
  obtain ⟨frame, facts, present, factsAt, _, _, _, _, outcome⟩ :=
    an_owned_occurrence_receipt_has_its_value_later_handoff_or_retirement
      (occurrence := 1) (key := jointReturnEntry.callKey) (actor := 0) (record := jointReturnReceipt)
      (binding := jointOccurrencePlan.path.bindings[1].2)
      actualJointRoundTripCustody
      (a_selected_visit_execution_has_its_same_physical_execution actualJointRoundTripSource)
      actualJointRoundTripOccurrence (by decide) (by decide)
      (by simp [jointRoundTripEvents, jointEvents, sourceWord, OwnedCallEvent.source?]) (by decide)
  have known : jointReturnReceipt.step.ownedFacts? (jointImages 0) custodyChoreo =
      some jointReturnReceiptFacts := (Option.some_get _).symm
  have same := Option.some.inj (factsAt.symm.trans known)
  subst facts
  have tag : (0, jointReturnReceiptFacts.afterView.header) = returnedValue := by decide
  rw [tag] at present outcome
  refine ⟨frame, present, ?_⟩
  rcases outcome with retained | released
  · exact retained
  · obtain ⟨earlier, later, event, before, after, wordAt, _, _, _, _, releaseAt, _⟩ := released
    have inWord : event ∈ jointRoundTripEvents := by
      rw [wordAt]
      exact List.mem_append_right earlier List.mem_cons_self
    have noRelease : (jointRoundTripEvents.all fun event =>
        decide (event.releasedValue? ≠ some returnedValue)) = true := by decide
    exact False.elim (of_decide_eq_true (List.all_eq_true.mp noRelease event inWord) releaseAt)

/-- The worker's actual release of the received tag is the later return
occurrence, not a second acceptance of the old loan. Its prefix still
contains the concrete receipt that issued this tag. -/
theorem actualJointReleaseHasItsLaterSourceOccurrence :
    ∃ frame earlier later entry request outgoing before after nextBinding,
      jointRoundTripEvents = earlier ++ OwnedCallEvent.accept entry request outgoing :: later ∧
      OwnedCallEvent.receive jointEntry.callKey 1 jointLoanReceipt frame receivedValue ∈ earlier ∧
      History jointImages jointResource custodyChoreo initialValue earlier before ∧
      Boundary jointImages jointResource custodyChoreo earlier (.accept entry request outgoing) before after ∧
      request.payload = receivedValue ∧
      occurrenceCall? [(jointEntry.callKey, 0), (jointReturnEntry.callKey, 1)] entry.callKey = some 1 ∧
      jointOccurrencePlan.path.bindings.find? (fun row => row.1 == 1) = some (1, nextBinding) ∧
      nextBinding.message.payload = .endpoint jointResource := by
  obtain ⟨frame, facts, _, factsAt, _, _, _, _, outcome⟩ :=
    an_owned_occurrence_receipt_has_its_value_later_handoff_or_retirement
      (occurrence := 0) (key := jointEntry.callKey) (actor := 1) (record := jointLoanReceipt)
      (binding := jointOccurrencePlan.path.bindings[0].2)
      actualJointRoundTripCustody
      (a_selected_visit_execution_has_its_same_physical_execution actualJointRoundTripSource)
      actualJointRoundTripOccurrence (by decide) (by decide)
      (by simp [jointRoundTripEvents, jointEvents, sourceWord, OwnedCallEvent.source?]) (by decide)
  have known : jointLoanReceipt.step.ownedFacts? (jointImages 1) custodyChoreo =
      some jointLoanReceiptFacts := (Option.some_get _).symm
  have same := Option.some.inj (factsAt.symm.trans known)
  subst facts
  have tag : (1, jointLoanReceiptFacts.afterView.header) = receivedValue := by decide
  rw [tag] at outcome
  rcases outcome with retained | ⟨earlier, later, event, before, after,
    wordAt, receiptAt, history, boundary, _, released, classified⟩
  · have absent : ¬ TransferValueLocated receivedValue jointReturnedCut.custody := by
      unfold TransferValueLocated
      decide
    exact False.elim (absent retained)
  · rcases classified with handoff | retired
    · obtain ⟨entry, request, outgoing, next, nextBinding, eventAt, forward, callAt, nextAt, payload⟩ := handoff
      subst event
      have bounds : (jointOccurrencePlan.path.bindings.all fun row =>
          decide (row.2.message.payload = .endpoint jointResource → row.1 ≤ 1)) = true := by decide
      have atMost : next ≤ 1 :=
        (of_decide_eq_true (List.all_eq_true.mp bounds _ (List.mem_of_find?_eq_some nextAt))) payload
      have nextIsReturn : next = 1 := by omega
      subst next
      exact ⟨frame, earlier, later, entry, request, outgoing, before, after, nextBinding,
        wordAt, receiptAt, history, boundary, Option.some.inj released, callAt, nextAt, payload⟩
    · obtain ⟨sample, recorded, _, _⟩ := retired
      have inWord : event ∈ jointRoundTripEvents := by
        rw [wordAt]
        exact List.mem_append_right earlier List.mem_cons_self
      have noneRetired : (jointRoundTripEvents.all fun event => decide (event.retirement? = none)) = true := by decide
      have absent := of_decide_eq_true (List.all_eq_true.mp noneRetired event inWord)
      rw [absent] at recorded
      contradiction

/-- Neither real owned receipt can be reclassified as ordinary traffic in
this captured prefix. The general theorem derives this classification from
the source and actual call; it does not assume an owned sender as input. -/
theorem actualJointOwnedReceiptsCannotHideAsOrdinaryBoundaries :
    ¬ Untracked jointResource custodyChoreo jointRoundTripEvents jointReturnedCut
      (.receive jointEntry.callKey 1 jointLoanReceipt) ∧
    ¬ Untracked jointResource custodyChoreo jointRoundTripEvents jointReturnedCut
      (.receive jointReturnEntry.callKey 0 jointReturnReceipt) := by
  have allTracked : (acceptedEntries jointRoundTripEvents).all (tracks jointResource custodyChoreo) = true := by
    decide
  constructor <;> intro ordinary
  all_goals
    obtain ⟨entry, member, _, notTracked⟩ := ordinary
    have actual := List.all_eq_true.mp allTracked entry member
    rw [actual] at notTracked
    contradiction

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
    a_joint_occurrence_execution_has_retained_resource_authority_and_source_progress
      actualJointRoundTripCustody
      (a_selected_visit_execution_has_its_same_physical_execution actualJointRoundTripSource)
      actualJointRoundTripOccurrence (by decide)
      (by
        have noneRetired : (jointRoundTripEvents.all fun event => decide (event.retirement? = none)) = true := by decide
        intro event present
        exact of_decide_eq_true (List.all_eq_true.mp noneRetired event present)) unfinished
  exact ⟨affine, rfl, history, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

/-- Derive current authority from source readiness and the SAME captured
round-trip word. The exact returned value is an output, not a final-cut
availability premise. Neither completed sender's witness remains authoritative. -/
theorem actualReadyReturnedReadDerivesItsCurrentAuthority :
    TransferValueLocated returnedValue jointReturnedCut.custody := by
  obtain ⟨value, actorAt, outcome⟩ :=
    a_ready_resource_operation_has_its_authority_or_checked_retirement
      (target := ⟨2, .send⟩) (binding := jointOccurrencePlan.path.bindings[2].2)
      (action := { actor := 0, endpoint := jointResource, side := .send })
      actualJointRoundTripCustody
      (a_selected_visit_execution_has_its_same_physical_execution actualJointRoundTripSource)
      actualJointRoundTripOccurrence (by decide) (by decide) (by decide) (by decide)
      (by decide) (by decide)
  rcases outcome with located | ⟨event, present, sample, retired, _, _⟩
  · have same : value = returnedValue := by
      rcases located with ⟨usable, _, _⟩ | ⟨empty, _, _⟩
      · change [returnedValue] = [value] at usable
        exact (List.cons.inj usable).1.symm
      · change [returnedValue] = [] at empty
        contradiction
    simpa only [same] using located
  · have noRetirement : (jointRoundTripEvents.all fun item => decide (item.retirement? = none)) = true := by decide
    have absent := of_decide_eq_true (List.all_eq_true.mp noRetirement event present)
    rw [absent] at retired
    contradiction

end
