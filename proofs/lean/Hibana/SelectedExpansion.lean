import Hibana.SourceResumptionProgress

namespace Hibana
open EndpointDelegation

private def key : EndpointKey := ⟨0, 3⟩
private def loan : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint key⟩
private def useResource : Message := ⟨⟨0, 3⟩, ⟨0, 1⟩, 4, .wire 0⟩
private def giveBack : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .endpoint key⟩
private def keep : Message := ⟨⟨0, 0⟩, ⟨0, 2⟩, 2, .wire 0⟩
private def leave : Message := ⟨⟨0, 0⟩, ⟨0, 2⟩, 3, .endpoint key⟩
private def body : Source := .seq (.send loan) (.seq (.send useResource) (.send giveBack))
private def continuation : Source := .route (some 17) (.send keep) (.send leave)
private def source : Source := .seq (.roll body) continuation
private def expanded : Source := .seq (.seq body continuation) body
private def initial : Normalization := (normalize source).get (by decide)
private def normalized : Normalization :=
  (normalizeExpansion initial.issuance expanded).get (by decide)
private def plan : SourceOccurrenceCut source := {
  original := initial
  originalAccepted := by decide
  expanded := expanded
  unfolding := .resumeAfterContinuation .hole
    (.seq (.rollOnce (SourceUnfolding.oneCopy body))
      (.route (some 17) (.send _) (.send _))) (SourceUnfolding.oneCopy body)
  normalized := normalized
  accepted := by decide
  sameIssuance := by decide
  path := normalized.paths[0]'(by decide)
  selected := by decide
  separated := by decide
  guarded := by decide
  observed := fun _ => false
}
private def observedPoints : List SourcePoint := [0, 1, 2, 3].flatMap fun event =>
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩]

theorem the_original_global_checks_both_alternatives :
    initial.paths.length = 2 := by decide

theorem the_expanded_tree_is_not_a_valid_all_alternatives_global :
    normalize expanded = none := by decide

theorem the_selected_expansion_keeps_the_original_initial_issuer :
    normalized.issuance = initial.issuance := by decide

theorem only_the_owner_preserving_selection_survives :
    normalized.paths.length = 1 ∧
      chosen? plan.path.selections ⟨3, 4, 5⟩ = some .left := by decide

theorem every_observed_prefix_step_is_ready :
    (observedPoints.zipIdx.all fun (point, index) =>
      (occurrenceCut plan (observedPoints.take index)).ready point) = true := by decide

theorem a_retained_loan_can_resume_after_the_owner_preserving_choice :
    ((occurrenceCut plan observedPoints).ready (.accepted 5) &&
      (occurrenceCut plan observedPoints).resumptionReady (.accepted 5)) = true := by decide

theorem the_unselected_transfer_cannot_enter_the_same_source_word :
    (occurrenceCut plan observedPoints).ready (.accepted 4) = false := by decide

theorem the_new_receipt_still_requires_its_own_acceptance :
    (occurrenceCut plan observedPoints).ready (.committed ⟨5, .receive⟩) = false := by decide

#print axioms the_original_global_checks_both_alternatives
#print axioms the_expanded_tree_is_not_a_valid_all_alternatives_global
#print axioms the_selected_expansion_keeps_the_original_initial_issuer
#print axioms only_the_owner_preserving_selection_survives
#print axioms every_observed_prefix_step_is_ready
#print axioms a_retained_loan_can_resume_after_the_owner_preserving_choice
#print axioms the_unselected_transfer_cannot_enter_the_same_source_word
#print axioms the_new_receipt_still_requires_its_own_acceptance

end Hibana
