import Hibana.SourceResumptionProgress

namespace Hibana
open EndpointDelegation

private def resource : EndpointKey := ⟨0, 3⟩
private def tick : Message := ⟨resource, ⟨0, 4⟩, 12, .wire 0⟩
private def tail : Message := ⟨⟨0, 0⟩, ⟨0, 2⟩, 13, .wire 0⟩
private def loan : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 14, .endpoint resource⟩
private def source : Source := .seq (.roll (.send tick)) (.seq (.send tail) (.send loan))
private def original : Normalization := (normalize source).get (by decide)
private def expanded : Source :=
  .seq (.send tick) (.seq (.par (.send tick) (.send tail)) (.send loan))
private def normalized : Normalization :=
  (normalizeExpansion original.issuance expanded).get (by decide)
private def plan : SourceOccurrenceCut source := {
  original := original
  originalAccepted := by decide
  expanded := expanded
  unfolding := .resumeAlongsideContinuation .hole (.seqLeft .hole (.send loan))
    (.rollOnce (.send tick)) (.rollOnce (.send tick)) (.seq (.send tail) (.send loan))
  normalized := normalized
  accepted := by decide
  sameIssuance := by decide
  path := normalized.paths[0]'(by decide)
  selected := by decide
  guarded := by decide
  observed := fun _ => false
}
private def beforeTail : List SourcePoint := [
  .accepted 0, .committed ⟨0, .receive⟩, .committed ⟨0, .send⟩,
  .accepted 1, .committed ⟨1, .receive⟩]

theorem PendingAcceptance.the_pending_continuation_has_its_source_prerequisites :
    (occurrenceCut plan beforeTail).ready (.accepted 2) = true := by decide

-- Actual Endpoint test:
-- a_pending_continuation_can_publish_after_the_other_view_reenters_its_roll.
-- Physical acceptance and the remote receipt precede Tail acceptance;
-- Tick's sender publication follows Tail publication.
theorem PendingAcceptance.unpublished_acceptance_does_not_enter_the_senders_roll :
    (occurrenceCut plan beforeTail).resumptionReady (.accepted 2) = true := by decide

#print axioms PendingAcceptance.the_pending_continuation_has_its_source_prerequisites
#print axioms PendingAcceptance.unpublished_acceptance_does_not_enter_the_senders_roll

private def done : Message := ⟨resource, resource, 15, .wire 0⟩
private def twoStepBody : Source := .seq (.send tick) (.send done)
private def twoStepSource : Source :=
  .seq (.roll twoStepBody) (.seq (.send tail) (.send loan))
private def twoStepOriginal : Normalization := (normalize twoStepSource).get (by decide)
private def twoStepExpanded : Source :=
  .seq twoStepBody (.seq (.par twoStepBody (.send tail)) (.send loan))
private def twoStepNormalized : Normalization :=
  (normalizeExpansion twoStepOriginal.issuance twoStepExpanded).get (by decide)
private def twoStepPlan : SourceOccurrenceCut twoStepSource := {
  original := twoStepOriginal
  originalAccepted := by decide
  expanded := twoStepExpanded
  unfolding := .resumeAlongsideContinuation .hole (.seqLeft .hole (.send loan))
    (.rollOnce (.oneCopy twoStepBody)) (.rollOnce (.oneCopy twoStepBody))
    (.seq (.send tail) (.send loan))
  normalized := twoStepNormalized
  accepted := by decide
  sameIssuance := by decide
  path := twoStepNormalized.paths[0]'(by decide)
  selected := by decide
  guarded := by decide
  observed := fun _ => false
}
private def publishedEntry : List SourcePoint := [
  .accepted 0, .committed ⟨0, .receive⟩, .committed ⟨0, .send⟩,
  .committed ⟨1, .local⟩,
  .accepted 2, .committed ⟨2, .receive⟩, .committed ⟨2, .send⟩]

theorem PendingAcceptance.a_committed_entry_still_waits_for_its_remaining_body :
    (occurrenceCut twoStepPlan publishedEntry).ready (.accepted 4) = true ∧
    (occurrenceCut twoStepPlan publishedEntry).resumptionReady (.accepted 4) = false := by decide

theorem PendingAcceptance.completing_that_body_enables_the_continuation :
    let observed := publishedEntry ++ [.committed ⟨3, .local⟩]
    (occurrenceCut twoStepPlan observed).ready (.accepted 4) = true ∧
    (occurrenceCut twoStepPlan observed).resumptionReady (.accepted 4) = true := by decide

#print axioms PendingAcceptance.a_committed_entry_still_waits_for_its_remaining_body
#print axioms PendingAcceptance.completing_that_body_enables_the_continuation
end Hibana
