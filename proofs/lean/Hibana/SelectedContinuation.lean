import Hibana.SourceResumptionProgress

namespace Hibana
open EndpointDelegation

private def resource : EndpointKey := ⟨0, 3⟩
private def loan : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint resource⟩
private def useResource : Message := ⟨resource, ⟨0, 1⟩, 4, .wire 0⟩
private def giveBack : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .endpoint resource⟩
private def keep : Message := ⟨⟨0, 2⟩, ⟨0, 0⟩, 2, .wire 0⟩
private def ask : Message := ⟨⟨0, 2⟩, ⟨0, 0⟩, 5, .wire 0⟩
private def leave : Message := ⟨⟨0, 0⟩, ⟨0, 2⟩, 3, .endpoint resource⟩
private def body : Source := .seq (.send loan) (.seq (.send useResource) (.send giveBack))
private def continuation : Source :=
  .route (some 17) (.send keep) (.seq (.send ask) (.send leave))
private def source : Source := .seq (.roll body) continuation
private def original : Normalization := (normalize source).get (by decide)

private def overlapping : Source := .seq body (.par body continuation)
private def overlappingNormalization : Normalization :=
  (normalizeExpansion original.issuance overlapping).get (by decide)
private def overlappingPlan : SourceOccurrenceCut source := {
  original := original
  originalAccepted := by decide
  expanded := overlapping
  unfolding := .resumeAlongsideContinuation .hole .hole
    (.rollOnce (.oneCopy body)) (.rollOnce (.oneCopy body)) (.oneCopy continuation)
  normalized := overlappingNormalization
  accepted := by decide
  sameIssuance := by decide
  path := overlappingNormalization.paths[0]'(by decide)
  selected := by decide
  guarded := by decide
  observed := fun _ => false
}
private def completeMessages (events : List Nat) : List SourcePoint := events.flatMap fun event =>
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩]
private def overlappingPoints : List SourcePoint := completeMessages [0, 1, 2, 3, 6, 4, 5]

private def insertion : RollContext :=
  .routeRight (some 17) (.send keep) (.seqRight (.send ask) .hole)
private def interrupted : Source := .seq body (insertion.plug (.seq body (.send leave)))
private def interruptedNormalization : Normalization :=
  (normalizeExpansion original.issuance interrupted).get (by decide)
private def interruptedPlan : SourceOccurrenceCut source := {
  original := original
  originalAccepted := by decide
  expanded := interrupted
  unfolding := .resumeBeforeContinuation .hole insertion
    (.rollOnce (.oneCopy body)) (.rollOnce (.oneCopy body)) (.oneCopy continuation)
  normalized := interruptedNormalization
  accepted := by decide
  sameIssuance := by decide
  path := interruptedNormalization.paths[1]'(by decide)
  selected := by decide
  guarded := by decide
  observed := fun _ => false
}
private def interruptedPoints : List SourcePoint := completeMessages [0, 1, 2, 4, 5, 6, 7, 8]

theorem a_selected_parallel_expansion_does_not_relax_original_global_ownership :
    original.paths.length = 2 ∧ normalize (.par body continuation) = none := by decide

theorem a_parallel_expansion_checks_only_its_selected_ownership_effects :
    overlappingNormalization.paths.length = 1 ∧
      chosen? overlappingPlan.path.selections ⟨6, 7, 9⟩ = some .left := by decide

theorem an_incoming_continuation_can_arrive_during_a_later_loan :
    (overlappingPoints.zipIdx.all fun (point, index) =>
      let prefixCut := occurrenceCut overlappingPlan (overlappingPoints.take index)
      prefixCut.ready point && prefixCut.resumptionReady point) = true := by decide

theorem a_retained_loan_can_enter_between_continuation_operations :
    interruptedPlan.unfolding.origins 0 = [0, 1, 2, 3, 4, 0, 1, 2, 5] ∧
      (interruptedPoints.zipIdx.all fun (point, index) =>
        let prefixCut := occurrenceCut interruptedPlan (interruptedPoints.take index)
        prefixCut.ready point && prefixCut.resumptionReady point) = true := by decide

theorem the_final_transfer_requires_the_resumed_loan_to_return :
    (occurrenceCut interruptedPlan (completeMessages [0, 1, 2, 4, 5])).ready (.accepted 8) = false :=
  by decide

theorem a_completed_transfer_cannot_be_followed_by_the_old_holders_loan :
    let expanded := .seq (.seq body continuation) body
    let normalized := (normalizeExpansion original.issuance expanded).get (by decide)
    normalized.paths.length = 1 ∧
      normalized.paths.all (fun path => chosen? path.selections ⟨3, 4, 6⟩ == some .left) = true :=
  by decide

#print axioms a_selected_parallel_expansion_does_not_relax_original_global_ownership
#print axioms a_parallel_expansion_checks_only_its_selected_ownership_effects
#print axioms an_incoming_continuation_can_arrive_during_a_later_loan
#print axioms a_retained_loan_can_enter_between_continuation_operations
#print axioms the_final_transfer_requires_the_resumed_loan_to_return
#print axioms a_completed_transfer_cannot_be_followed_by_the_old_holders_loan

end Hibana
