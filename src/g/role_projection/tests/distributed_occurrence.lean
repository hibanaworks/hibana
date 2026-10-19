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

theorem distributedLocalProjectionReconstructsItsOccurrenceHistory :
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
  exact h24

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

end
