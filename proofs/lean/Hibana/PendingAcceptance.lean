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
  unfolding := .resumeAlongsideContinuation .hole
    (.seqRight (.send tick) (.seqLeft .hole (.send loan)))
    (.seq (.rollOnce (.send tick)) (.seq (.send tail) (.send loan))) (.rollOnce (.send tick))
  normalized := normalized
  accepted := by decide
  sameIssuance := by decide
  path := normalized.paths[0]'(by decide)
  selected := by decide
  separated := by decide
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
  unfolding := .resumeAlongsideContinuation .hole
    (.seqRight twoStepBody (.seqLeft .hole (.send loan)))
    (.seq (.rollOnce (.oneCopy twoStepBody)) (.seq (.send tail) (.send loan)))
    (.rollOnce (.oneCopy twoStepBody))
  normalized := twoStepNormalized
  accepted := by decide
  sameIssuance := by decide
  path := twoStepNormalized.paths[0]'(by decide)
  selected := by decide
  separated := by decide
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

-- A delegated file and its holder are different views of one actor. The
-- second visit cannot publish until both current receipts have committed.
-- This checks the source cut; the public Endpoint regression separately
-- distinguishes a legal suspended call from an immediate rejection.
private def fileLoan : Message := ⟨⟨0, 2⟩, ⟨0, 0⟩, 30, .endpoint resource⟩
private def fileRequest : Message := ⟨resource, ⟨0, 1⟩, 31, .wire 0⟩
private def holderReply : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 32, .wire 0⟩
private def fileReply : Message := ⟨⟨0, 1⟩, resource, 33, .wire 0⟩
private def fileBody : Source :=
  .seq (.send fileRequest) (.par (.send holderReply) (.send fileReply))
private def fileSource : Source := .seq (.send fileLoan) (.roll fileBody)
private def fileOriginal : Normalization := (normalize fileSource).get (by decide)
private def fileExpanded : Source := .seq (.send fileLoan) (.seq fileBody fileBody)
private def fileNormalized : Normalization :=
  (normalizeExpansion fileOriginal.issuance fileExpanded).get (by decide)
private def filePlan : SourceOccurrenceCut fileSource := {
  original := fileOriginal
  originalAccepted := by decide
  expanded := fileExpanded
  unfolding := .seq (.send fileLoan)
    (.rollMore (.rollOnce (.oneCopy fileBody)) (.oneCopy fileBody))
  normalized := fileNormalized
  accepted := by decide
  sameIssuance := by decide
  path := fileNormalized.paths[0]'(by decide)
  selected := by decide
  separated := by decide
  guarded := by decide
  observed := fun _ => false
}
private def beforeParallelReceipts : List SourcePoint := [
  .accepted 0, .committed ⟨0, .send⟩, .committed ⟨0, .receive⟩,
  .accepted 1, .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩,
  .accepted 2, .committed ⟨2, .send⟩,
  .accepted 3, .committed ⟨3, .send⟩]

theorem PendingAcceptance.next_file_visit_waits_for_the_holders_current_receipt :
    (occurrenceCut filePlan (beforeParallelReceipts ++ [.committed ⟨3, .receive⟩])).ready
      (.accepted 4) = false := by decide

theorem PendingAcceptance.next_file_visit_waits_for_its_own_current_receipt :
    (occurrenceCut filePlan (beforeParallelReceipts ++ [.committed ⟨2, .receive⟩])).ready
      (.accepted 4) = false := by decide

theorem PendingAcceptance.both_current_receipts_enable_the_next_file_visit :
    (occurrenceCut filePlan (beforeParallelReceipts ++
      [.committed ⟨3, .receive⟩, .committed ⟨2, .receive⟩])).ready (.accepted 4) = true := by decide

#print axioms PendingAcceptance.next_file_visit_waits_for_the_holders_current_receipt
#print axioms PendingAcceptance.next_file_visit_waits_for_its_own_current_receipt
#print axioms PendingAcceptance.both_current_receipts_enable_the_next_file_visit
end Hibana
