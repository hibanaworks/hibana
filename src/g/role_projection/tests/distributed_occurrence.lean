section
open Hibana Hibana.EndpointDelegation

private def distributedBorrowBody : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint ⟨0, 10⟩⟩)
  (.seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 9, .wire 6⟩)
    (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .endpoint ⟨0, 10⟩⟩))
private def distributedReadTail : Source := .seq
  (.send ⟨⟨0, 10⟩, ⟨0, 3⟩, 3, .wire 6⟩)
  (.send ⟨⟨0, 3⟩, ⟨0, 10⟩, 4, .wire 6⟩)

def distributedOccurrenceSource : Source := (distributedChoreo.ownershipSource?).get (by decide)
def distributedOccurrenceOriginal : Normalization := (normalize distributedOccurrenceSource).get (by decide)
def distributedOccurrenceExpanded : Source :=
  .seq (.seq distributedBorrowBody distributedBorrowBody) distributedReadTail
def distributedOccurrenceNormalized : Normalization :=
  (normalize distributedOccurrenceExpanded).get (by decide)
def distributedOccurrencePlan : SourceOccurrenceCut distributedOccurrenceSource := {
  original := distributedOccurrenceOriginal
  originalAccepted := (Option.some_get _).symm
  expanded := distributedOccurrenceExpanded
  unfolding := .seq
    (.rollMore (.rollOnce (SourceUnfolding.oneCopy distributedBorrowBody))
      (SourceUnfolding.oneCopy distributedBorrowBody))
    (SourceUnfolding.oneCopy distributedReadTail)
  normalized := distributedOccurrenceNormalized
  accepted := (Option.some_get _).symm
  sameIssuance := by decide
  path := distributedOccurrenceNormalized.paths[0]
  selected := by decide
  observed := fun _ => false
}

/-- All eight physical acceptances and sixteen measured commits, in the same
order as the existing captured execution. Ordinary Note/Read/Value frames are
retained; equal loan envelopes are resolved by their actual carrier call keys. -/
def distributedOccurrenceWord : List DistributedCallBoundary := [
  .accept distributedEntry0, .publish distributedEntry0.callKey 0 distributedCommit0,
  .accept distributedEntry1, .publish distributedEntry1.callKey 0 distributedCommit1,
  .receive distributedEntry0.callKey 1 distributedCommit2,
  .receive distributedEntry1.callKey 1 distributedCommit3,
  .accept distributedEntry4, .publish distributedEntry4.callKey 1 distributedCommit4,
  .receive distributedEntry4.callKey 0 distributedCommit5,
  .accept distributedEntry6, .publish distributedEntry6.callKey 0 distributedCommit6,
  .accept distributedEntry7, .publish distributedEntry7.callKey 0 distributedCommit7,
  .receive distributedEntry6.callKey 1 distributedCommit8,
  .receive distributedEntry7.callKey 1 distributedCommit9,
  .accept distributedEntry10, .publish distributedEntry10.callKey 1 distributedCommit10,
  .receive distributedEntry10.callKey 0 distributedCommit11,
  .accept distributedEntry12, .publish distributedEntry12.callKey 0 distributedCommit12,
  .receive distributedEntry12.callKey 3 distributedCommit13,
  .accept distributedEntry14, .publish distributedEntry14.callKey 3 distributedCommit14,
  .receive distributedEntry14.callKey 0 distributedCommit15]

def distributedOccurrenceCalls : OccurrenceCalls := [
  (distributedEntry0.callKey, 0), (distributedEntry1.callKey, 1),
  (distributedEntry4.callKey, 2), (distributedEntry6.callKey, 3),
  (distributedEntry7.callKey, 4), (distributedEntry10.callKey, 5),
  (distributedEntry12.callKey, 6), (distributedEntry14.callKey, 7)]

def distributedOccurrencePoints : List SourcePoint := [
  .accepted 0, .committed ⟨0, .send⟩, .accepted 1, .committed ⟨1, .send⟩,
  .committed ⟨0, .receive⟩, .committed ⟨1, .receive⟩,
  .accepted 2, .committed ⟨2, .send⟩, .committed ⟨2, .receive⟩,
  .accepted 3, .committed ⟨3, .send⟩, .accepted 4, .committed ⟨4, .send⟩,
  .committed ⟨3, .receive⟩, .committed ⟨4, .receive⟩,
  .accepted 5, .committed ⟨5, .send⟩, .committed ⟨5, .receive⟩,
  .accepted 6, .committed ⟨6, .send⟩, .committed ⟨6, .receive⟩,
  .accepted 7, .committed ⟨7, .send⟩, .committed ⟨7, .receive⟩]

private theorem distributedCapturedOccurrencePrefixes :
    SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan
      (distributedOccurrenceWord.take 9) (distributedOccurrenceCalls.take 3)
      (distributedOccurrencePoints.take 9) ∧
    SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan
      (distributedOccurrenceWord.take 18) (distributedOccurrenceCalls.take 6)
      (distributedOccurrencePoints.take 18) ∧
    SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan
      distributedOccurrenceWord distributedOccurrenceCalls distributedOccurrencePoints := by
  have h0 : SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan [] [] [] :=
    .nil (by decide)
  have h1 := SourceOccurrenceTrace.accept (entry := distributedEntry0) (occurrence := 0) h0
    (by decide) (by decide) (by decide) (by decide)
  have h2 := SourceOccurrenceTrace.publish (key := distributedEntry0.callKey) (actor := 0)
    (record := distributedCommit0) (occurrence := 0) h1 (by decide) (by decide) (by decide)
  have h3 := SourceOccurrenceTrace.accept (entry := distributedEntry1) (occurrence := 1) h2
    (by decide) (by decide) (by decide) (by decide)
  have h4 := SourceOccurrenceTrace.publish (key := distributedEntry1.callKey) (actor := 0)
    (record := distributedCommit1) (occurrence := 1) h3 (by decide) (by decide) (by decide)
  have h5 := SourceOccurrenceTrace.receive (key := distributedEntry0.callKey) (actor := 1)
    (record := distributedCommit2) (occurrence := 0) h4 (by decide) (by decide) (by decide)
  have h6 := SourceOccurrenceTrace.receive (key := distributedEntry1.callKey) (actor := 1)
    (record := distributedCommit3) (occurrence := 1) h5 (by decide) (by decide) (by decide)
  have h7 := SourceOccurrenceTrace.accept (entry := distributedEntry4) (occurrence := 2) h6
    (by decide) (by decide) (by decide) (by decide)
  have h8 := SourceOccurrenceTrace.publish (key := distributedEntry4.callKey) (actor := 1)
    (record := distributedCommit4) (occurrence := 2) h7 (by decide) (by decide) (by decide)
  have h9 := SourceOccurrenceTrace.receive (key := distributedEntry4.callKey) (actor := 0)
    (record := distributedCommit5) (occurrence := 2) h8 (by decide) (by decide) (by decide)
  have h10 := SourceOccurrenceTrace.accept (entry := distributedEntry6) (occurrence := 3) h9
    (by decide) (by decide) (by decide) (by decide)
  have h11 := SourceOccurrenceTrace.publish (key := distributedEntry6.callKey) (actor := 0)
    (record := distributedCommit6) (occurrence := 3) h10 (by decide) (by decide) (by decide)
  have h12 := SourceOccurrenceTrace.accept (entry := distributedEntry7) (occurrence := 4) h11
    (by decide) (by decide) (by decide) (by decide)
  have h13 := SourceOccurrenceTrace.publish (key := distributedEntry7.callKey) (actor := 0)
    (record := distributedCommit7) (occurrence := 4) h12 (by decide) (by decide) (by decide)
  have h14 := SourceOccurrenceTrace.receive (key := distributedEntry6.callKey) (actor := 1)
    (record := distributedCommit8) (occurrence := 3) h13 (by decide) (by decide) (by decide)
  have h15 := SourceOccurrenceTrace.receive (key := distributedEntry7.callKey) (actor := 1)
    (record := distributedCommit9) (occurrence := 4) h14 (by decide) (by decide) (by decide)
  have h16 := SourceOccurrenceTrace.accept (entry := distributedEntry10) (occurrence := 5) h15
    (by decide) (by decide) (by decide) (by decide)
  have h17 := SourceOccurrenceTrace.publish (key := distributedEntry10.callKey) (actor := 1)
    (record := distributedCommit10) (occurrence := 5) h16 (by decide) (by decide) (by decide)
  have h18 := SourceOccurrenceTrace.receive (key := distributedEntry10.callKey) (actor := 0)
    (record := distributedCommit11) (occurrence := 5) h17 (by decide) (by decide) (by decide)
  have h19 := SourceOccurrenceTrace.accept (entry := distributedEntry12) (occurrence := 6) h18
    (by decide) (by decide) (by decide) (by decide)
  have h20 := SourceOccurrenceTrace.publish (key := distributedEntry12.callKey) (actor := 0)
    (record := distributedCommit12) (occurrence := 6) h19 (by decide) (by decide) (by decide)
  have h21 := SourceOccurrenceTrace.receive (key := distributedEntry12.callKey) (actor := 3)
    (record := distributedCommit13) (occurrence := 6) h20 (by decide) (by decide) (by decide)
  have h22 := SourceOccurrenceTrace.accept (entry := distributedEntry14) (occurrence := 7) h21
    (by decide) (by decide) (by decide) (by decide)
  have h23 := SourceOccurrenceTrace.publish (key := distributedEntry14.callKey) (actor := 3)
    (record := distributedCommit14) (occurrence := 7) h22 (by decide) (by decide) (by decide)
  have h24 := SourceOccurrenceTrace.receive (key := distributedEntry14.callKey) (actor := 0)
    (record := distributedCommit15) (occurrence := 7) h23 (by decide) (by decide) (by decide)
  exact ⟨h9, h18, h24⟩

theorem distributedLocalProjectionReconstructsItsOccurrenceHistory :
    SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan
      distributedOccurrenceWord distributedOccurrenceCalls distributedOccurrencePoints :=
  distributedCapturedOccurrencePrefixes.2.2

private def distributedOccurrenceElaboration :=
  (elaborate distributedOccurrencePlan.expanded.resources
    (initialOwners distributedOccurrencePlan.expanded.resources)
    distributedOccurrencePlan.path.selections distributedOccurrencePlan.expanded 0).get (by decide)

private def distributedFirstSymbolicBinding := distributedOccurrenceElaboration.bindings[0].2

private theorem distributedResourceOriginAt
    {word calls points target binding action}
    (trace : SourceOccurrenceTrace distributedImages distributedChoreo
      distributedOccurrencePlan word calls points)
    (located : distributedOccurrencePlan.path.bindings.find?
      (fun entry => entry.1 == target.event) = some (target.event, binding))
    (decoded : action? distributedOccurrencePlan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = ⟨0, 10⟩
      | .send => binding.message.sender = ⟨0, 10⟩ ∨ binding.message.payload = .endpoint ⟨0, 10⟩
      | .local => binding.message.sender = ⟨0, 10⟩)
    (ready : occurrenceLocalReady distributedOccurrencePlan action.actor
      (occurrenceLocalHistory distributedOccurrencePlan action.actor points) target.entryPoint = true)
    (wire : occurrenceWireReady points target.entryPoint = true)
    (notFirst : target.event ≠ 0) (actorAt : action.actor = 0) :
    ∃ occurrence key record,
      SourcePointEdge distributedOccurrencePlan.expanded distributedOccurrencePlan.path.bindings
        (.committed ⟨occurrence, .receive⟩) target.entryPoint ∧
      points.contains (.committed ⟨occurrence, .receive⟩) = true ∧
      .receive key 0 record ∈ word ∧
      distributedOccurrencePlan.matchesAction distributedImage0 distributedChoreo 0
        record.step.pathIndex record.step.event occurrence .receive = true ∧
      occurrenceCall? calls key = some occurrence := by
  obtain ⟨run, index, symbolic, elaborated, indexAt, _, _, _, origin⟩ :=
    a_ready_occurrence_resource_has_its_initial_owner_or_same_word_receipt
      trace located decoded required ready wire
  rcases origin with initial | received
  · have actualRun : run = distributedOccurrenceElaboration := Option.some.inj
      (elaborated.symm.trans (Option.some_get _).symm)
    subst run
    cases index with
    | zero =>
      have firstAt : distributedOccurrenceElaboration.bindings[0]? =
          some (0, distributedFirstSymbolicBinding) := by decide
      have same := Option.some.inj (indexAt.symm.trans firstAt)
      exact False.elim (notFirst (congrArg Prod.fst same))
    | succ index =>
      have firstAt : distributedOccurrenceElaboration.bindings[0]? =
          some (0, distributedFirstSymbolicBinding) := by decide
      have firstIn : (0, distributedFirstSymbolicBinding) ∈
          distributedOccurrenceElaboration.bindings.take (index + 1) := by
        apply List.mem_of_getElem? (i := 0)
        simpa only [List.getElem?_take, Nat.zero_lt_succ, ite_true] using firstAt
      exact False.elim (initial.2 _ firstIn (by decide))
  · obtain ⟨occurrence, previousBinding, previousAction, earlier, previous, later, key, record,
      _, _, _, _, _, _, _, _, edge, observed, inWord, matched, callAt⟩ := received
    exact ⟨occurrence, key, record, edge, observed,
      by simpa only [actorAt] using inWord, by simpa only [actorAt] using matched, callAt⟩

/-- Fresh loan preparation and the later DB use require the received resource
from these actual prefixes. Initial issuance cannot replace either return.
The general origin theorem retains the last-handoff interval; this checks it
on the same two-visit Endpoint execution, without a new fixture or owner flag. -/
theorem distributedFreshResourceUsesHaveSameWordReceipts :
    (∃ occurrence key record,
      SourcePointEdge distributedOccurrencePlan.expanded distributedOccurrencePlan.path.bindings
        (.committed ⟨occurrence, .receive⟩) (.accepted 3) ∧
      (distributedOccurrencePoints.take 9).contains (.committed ⟨occurrence, .receive⟩) = true ∧
      .receive key 0 record ∈ distributedOccurrenceWord.take 9 ∧
      distributedOccurrencePlan.matchesAction distributedImage0 distributedChoreo 0
        record.step.pathIndex record.step.event occurrence .receive = true ∧
      occurrenceCall? (distributedOccurrenceCalls.take 3) key = some occurrence) ∧
    (∃ occurrence key record,
      SourcePointEdge distributedOccurrencePlan.expanded distributedOccurrencePlan.path.bindings
        (.committed ⟨occurrence, .receive⟩) (.accepted 6) ∧
      (distributedOccurrencePoints.take 18).contains (.committed ⟨occurrence, .receive⟩) = true ∧
      .receive key 0 record ∈ distributedOccurrenceWord.take 18 ∧
      distributedOccurrencePlan.matchesAction distributedImage0 distributedChoreo 0
        record.step.pathIndex record.step.event occurrence .receive = true ∧
      occurrenceCall? (distributedOccurrenceCalls.take 6) key = some occurrence) := by
  constructor
  · exact distributedResourceOriginAt (target := ⟨3, .send⟩)
      (binding := distributedOccurrencePlan.path.bindings[3].2)
      (action := { actor := 0, endpoint := ⟨0, 0⟩, side := .send })
      distributedCapturedOccurrencePrefixes.1 (by decide) (by decide) (by decide)
      (by decide) (by decide) (by decide) rfl

  · exact distributedResourceOriginAt (target := ⟨6, .send⟩)
      (binding := distributedOccurrencePlan.path.bindings[6].2)
      (action := { actor := 0, endpoint := ⟨0, 10⟩, side := .send })
      distributedCapturedOccurrencePrefixes.2.1 (by decide) (by decide) (by decide)
      (by decide) (by decide) (by decide) rfl

/-- Equal static receive identity is deliberately insufficient: the first
return still matches the repeated source row, but its actual call resolves to
occurrence 2, not the second return at occurrence 5. -/
theorem distributedResourceOriginRejectsAnOldEqualReceipt :
    distributedOccurrencePlan.matchesAction distributedImage0 distributedChoreo 0
      distributedCommit5.step.pathIndex distributedCommit5.step.event 5 .receive = true ∧
    occurrenceCall? (distributedOccurrenceCalls.take 6) distributedEntry4.callKey = some 2 ∧
    occurrenceCall? (distributedOccurrenceCalls.take 6) distributedEntry4.callKey ≠ some 5 := by
  decide

/-- No ordinary frame or measured commit was dropped from this qualification.
Both loan visits and the later resource operation finish in the source model.
This does not claim that every production history has such a finite unfolding. -/
theorem distributedProjectedHistoryIncludesEveryPhysicalBoundary :
    distributedOccurrenceWord.filterMap DistributedCallBoundary.record? = distributedRecords ∧
      distributedOccurrenceWord.filterMap DistributedCallBoundary.acceptance? = distributedEntries24 ∧
      (sourceVisitPoints distributedOccurrencePlan.path.bindings).all
        (fun point => distributedOccurrencePoints.contains point) = true ∧
      SourceOccurrenceExecution (occurrenceCut distributedOccurrencePlan [])
        (occurrenceCut distributedOccurrencePlan distributedOccurrencePoints) distributedOccurrencePoints :=
  ⟨rfl, rfl, by decide,
    a_source_occurrence_trace_is_its_reconstructed_execution
      distributedLocalProjectionReconstructsItsOccurrenceHistory⟩

/-- Four real raw queries: first-visit loan/note and fresh-visit loan/note.
The source points are prefixes of the same complete captured word above.
Each result uses the authentic current or purely prepared fresh base. -/
theorem distributedProjectedOccurrencesReachTheirActualCursorQueries :
    distributedCandidate0.admitted[0]? = some true ∧
      distributedCandidate1.admitted[1]? = some true ∧
      distributedCandidate2.admitted[0]? = some true ∧
      distributedCandidate3.admitted[1]? = some true := by
  have firstLoan := a_projected_occurrence_has_actual_candidate_admission
    (sample := distributedCandidate0) (image := distributedImage0) (choreo := distributedChoreo)
    (previousPath := 0) (data := distributedCandidateData) (path := distributedCandidatePath)
    (row := distributedCandidateRow0) (base := distributedCandidateBase0)
    (plan := distributedOccurrencePlan) (points := distributedOccurrencePoints.take 0)
    (target := ⟨0, .send⟩)
    (by decide) ((Option.some_get (x := distributedImage0.decodeActorOrder? distributedChoreo) (by decide)).symm)
    (by decide) (by decide) (.current (by decide))
    (by decide) (by decide) (by decide)
  have firstNote := a_projected_occurrence_has_actual_candidate_admission
    (sample := distributedCandidate1) (image := distributedImage0) (choreo := distributedChoreo)
    (previousPath := 0) (data := distributedCandidateData) (path := distributedCandidatePath)
    (row := distributedCandidateRow1) (base := distributedCandidateBase1)
    (plan := distributedOccurrencePlan) (points := distributedOccurrencePoints.take 2)
    (target := ⟨1, .send⟩)
    (by decide) ((Option.some_get (x := distributedImage0.decodeActorOrder? distributedChoreo) (by decide)).symm)
    (by decide) (by decide) (.current (by decide))
    (by decide) (by decide) (by decide)
  have freshLoan := a_projected_occurrence_has_actual_candidate_admission
    (sample := distributedCandidate2) (image := distributedImage0) (choreo := distributedChoreo)
    (previousPath := 0) (data := distributedCandidateData) (path := distributedCandidatePath)
    (row := distributedCandidateRow2) (base := distributedCandidateBase2)
    (plan := distributedOccurrencePlan) (points := distributedOccurrencePoints.take 9)
    (target := ⟨3, .send⟩)
    (by decide) ((Option.some_get (x := distributedImage0.decodeActorOrder? distributedChoreo) (by decide)).symm)
    (by decide) (by decide)
    (.fresh (rolls := distributedCandidateRolls) (roll := distributedCandidateRoll)
      (by decide) (by decide) (by decide))
    (by decide) (by decide) (by decide)
  have freshNote := a_projected_occurrence_has_actual_candidate_admission
    (sample := distributedCandidate3) (image := distributedImage0) (choreo := distributedChoreo)
    (previousPath := 0) (data := distributedCandidateData) (path := distributedCandidatePath)
    (row := distributedCandidateRow3) (base := distributedCandidateBase3)
    (plan := distributedOccurrencePlan) (points := distributedOccurrencePoints.take 11)
    (target := ⟨4, .send⟩)
    (by decide) ((Option.some_get (x := distributedImage0.decodeActorOrder? distributedChoreo) (by decide)).symm)
    (by decide) (by decide) (.current (by decide))
    (by decide) (by decide) (by decide)
  exact ⟨firstLoan.2, firstNote.2, freshLoan.2, freshNote.2⟩

/-- Old completed bits and an old equal send cannot replace the current
occurrence. These are negative controls for the actual fresh-loan/note cuts. -/
theorem distributedCandidateCorrespondenceRejectsOldProgress :
    distributedOccurrencePlan.candidateHistoryMatches distributedImage0 distributedChoreo 0
      distributedCandidateData distributedCandidatePath distributedCandidateRow2
      distributedCandidate2.snapshot (distributedOccurrencePoints.take 9) ⟨3, .send⟩ = false ∧
    distributedOccurrencePlan.candidateHistoryMatches distributedImage0 distributedChoreo 0
      distributedCandidateData distributedCandidatePath distributedCandidateRow3
      distributedCandidateBase3
      ((distributedOccurrencePoints.take 11).filter fun point =>
        decide (point ≠ .committed ⟨3, .send⟩)) ⟨4, .send⟩ = false := by
  decide

/-- The same captured two-visit word retains the first loan receipt before
the second loan, and the first return receipt before the second return.
These are consequences of source causality and the actual trace, rather
than independent assumptions about a final ownership bitmap. -/
theorem distributedRepeatedHandoffsRetainTheirCausalReceipts :
    SourcePoint.committed ⟨0, .receive⟩ ∈ distributedOccurrencePoints ∧
      SourcePoint.committed ⟨2, .receive⟩ ∈ distributedOccurrencePoints.take 18 ∧
      (occurrenceCut distributedOccurrencePlan distributedOccurrencePoints).ready
        (.accepted 0) = false ∧
      (occurrenceCut distributedOccurrencePlan (distributedOccurrencePoints.take 18)).ready
        (.accepted 2) = false := by
  have loan := a_recorded_handoff_keeps_earlier_receipts_and_excludes_old_acceptances
    (later := 3) (earlier := 0) (resource := ⟨0, 10⟩)
    (binding := distributedOccurrencePlan.path.bindings[3].2)
    (previous := distributedOccurrencePlan.path.bindings[0].2)
    (sender := ⟨0, ⟨0, 0⟩, .send⟩)
    distributedCapturedOccurrencePrefixes.2.2
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)
  have returned := a_recorded_handoff_keeps_earlier_receipts_and_excludes_old_acceptances
    (later := 5) (earlier := 2) (resource := ⟨0, 10⟩)
    (binding := distributedOccurrencePlan.path.bindings[5].2)
    (previous := distributedOccurrencePlan.path.bindings[2].2)
    (sender := ⟨1, ⟨0, 1⟩, .send⟩)
    distributedCapturedOccurrencePrefixes.2.1
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)
  exact ⟨loan.1, returned.1, loan.2, returned.2⟩

/-- Keeping the second return while forgetting the first return's receipt
cannot describe this chronological execution, even though their envelopes
have the same static identity. -/
theorem distributedRepeatedHandoffsRejectAForgottenReceipt :
    ¬ SourceOccurrenceTrace distributedImages distributedChoreo distributedOccurrencePlan
      (distributedOccurrenceWord.take 18) (distributedOccurrenceCalls.take 6)
      ((distributedOccurrencePoints.take 18).filter fun point =>
        decide (point ≠ .committed ⟨2, .receive⟩)) := by
  intro trace
  have kept := a_recorded_handoff_keeps_earlier_receipts_and_excludes_old_acceptances
    (later := 5) (earlier := 2) (resource := ⟨0, 10⟩) trace
    (binding := distributedOccurrencePlan.path.bindings[5].2)
    (previous := distributedOccurrencePlan.path.bindings[2].2)
    (sender := ⟨1, ⟨0, 1⟩, .send⟩)
    (by decide) (by decide) (by decide) (by decide) (by decide) (by decide) (by decide)
  have absent : SourcePoint.committed ⟨2, .receive⟩ ∉
      ((distributedOccurrencePoints.take 18).filter fun point =>
        decide (point ≠ .committed ⟨2, .receive⟩)) := by decide
  exact absent kept.1

/-- The actual second loan is accepted after the first return receipt. Equal
rolled envelope bytes do not let that acceptance reuse an older occurrence. -/
theorem distributedFreshLoanHasItsLaterHandoffOccurrence :
    ∃ binding,
      occurrenceCall? distributedOccurrenceCalls distributedEntry6.callKey = some 3 ∧
      distributedOccurrencePlan.path.bindings.find? (fun row => row.1 == 3) = some (3, binding) ∧
      binding.message.payload = .endpoint ⟨0, 10⟩ ∧
      distributedEntry0.sample.wire.payload = distributedEntry6.sample.wire.payload ∧ 2 < 3 := by
  have accepted := SourceOccurrenceTrace.accept (entry := distributedEntry6) (occurrence := 3)
    distributedCapturedOccurrencePrefixes.1 (by decide) (by decide) (by decide) (by decide)
  obtain ⟨next, binding, forward, callAt, nextAt, payload⟩ :=
    a_handoff_after_a_resource_receipt_has_a_later_occurrence
      (receipt := 2) (key := distributedEntry4.callKey) (actor := 0) (record := distributedCommit5)
      (receiptBinding := distributedOccurrencePlan.path.bindings[2].2)
      (facts := distributedFreshSentFacts) (resource := ⟨0, 10⟩)
      (finalCalls := distributedOccurrenceCalls)
      accepted ⟨distributedOccurrenceCalls.drop 4, by decide⟩
      (List.mem_of_getElem? (i := 8) rfl)
      (by decide) (by decide) (by decide) (Option.some_get _).symm (by decide) (by decide)
  have nextIsFresh : next = 3 := Option.some.inj (callAt.symm.trans (by decide))
  subst next
  exact ⟨binding, callAt, nextAt, payload, by decide, forward⟩

end
