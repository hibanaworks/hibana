import Hibana.SourceReadyReceipt
import Hibana.ActorMutationCoverage

namespace Hibana
open EndpointDelegation

private theorem a_handoff_at_or_before_a_completed_receipt_is_complete
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer receipt : DecodedActorRow}
    {start : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path)
    (receiptMember : receipt ∈ data.activeRows path)
    (sending : transfer.actionId.side = .send) (receiving : receipt.actionId.side = .receive)
    (ordered : data.source.before transfer.actionId.event receipt.actionId.event = true ∨
      transfer.actionId.event = receipt.actionId.event)
    (completed : start.doneState.done receipt.eventId = true)
    (closed : start.doneState.PrefixClosed (data.sourcePredecessors path) [receipt.eventId]) :
    start.doneState.done transfer.eventId = true := by
  have left := active_decoded_row_has_its_exact_source_action checked decoded selected transferMember
  have right := active_decoded_row_has_its_exact_source_action checked decoded selected receiptMember
  have before : precedes data.source path.bindings transfer.actionId receipt.actionId = true := by
    rcases ordered with causal | same
    · exact source_order_gives_actor_precedence left right causal
    · have sameActor : (data.actor == data.actor) = true := decide_eq_true rfl
      have sameEvent : (transfer.actionId.event == receipt.actionId.event) = true := decide_eq_true same
      simp only [precedes, left, right, DecodedActorRow.sourceAction, sameActor,
        Bool.true_and, sameEvent, sending, receiving, ite_true]
      decide
  exact closed receipt.eventId List.mem_cons_self completed transfer.eventId
    (actor_precedence_is_a_source_predecessor transferMember receiptMember before)

/-- Before the first source handoff, a measured pending target excludes any
other handoff at an earlier cut. There is no assumption that the target was
ready at that cut and no resource-value premise. -/
theorem a_measured_initial_resource_interval_excludes_a_distinct_handoff
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {target transfer : DecodedActorRow} {targetBinding loan : Binding}
    {key : EndpointKey} {issuance : List Issuance} {run : Elaboration} {index : Nat}
    {targetSymbolic : SymbolicBinding}
    {cut final : ActorProgressObservation} {cutCount finalCount : Nat}
    {later : List ActorCommitMutationRecord}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (targetMember : target ∈ data.activeRows path)
    (transferMember : transfer ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (transferred : loan.message.payload = .endpoint key)
    (used : targetBinding.message.sender = key ∨ targetBinding.message.receiver = key ∨
      targetBinding.message.payload = .endpoint key)
    (different : transfer.actionId.event ≠ target.actionId.event)
    (elaborated : elaborate data.source.resources (initialOwners data.source.resources)
      path.selections data.source 0 = some run)
    (sourceTarget : run.bindings[index]? = some (target.actionId.event, targetSymbolic))
    (targetInstantiated : instantiateBinding issuance targetSymbolic = some targetBinding)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some path.bindings)
    (untouched : ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint key)
    (suffixChecked : actorCommitMutationHistoryCheck image choreo session
      cut cutCount final finalCount later = true)
    (pending : final.doneState.done target.eventId = false) :
    sourceRowReady data path cut transfer = false := by
  obtain ⟨sent, sentMember, sentAt⟩ := instantiated_path_binding_has_source_input mapped loanAt
  have sentMessage := (instantiated_binding_retains_message_and_actor_references sentAt).1
  have targetMessage := (instantiated_binding_retains_message_and_actor_references targetInstantiated).1
  have order := an_initial_resource_has_no_handoff_before_its_target elaborated sourceTarget
    untouched sentMember (sentMessage ▸ transferred) (targetMessage ▸ used)
  rcases order with same | after
  · exact False.elim (different same)
  · have targetAction := active_decoded_row_has_its_exact_source_action checked decoded selected targetMember
    have transferAction := active_decoded_row_has_its_exact_source_action checked decoded selected transferMember
    exact a_pending_source_predecessor_blocks_its_successor
      (actor_precedence_is_a_source_predecessor targetMember transferMember
        (source_order_gives_actor_precedence targetAction transferAction after))
      (a_pending_final_event_was_pending_at_every_measured_commit suffixChecked pending).1

/-- A last source receipt and actual measured progress exclude every distinct
handoff until the target. The target need only be pending at the final cut:
it need not have been ready during earlier independent parallel work.

The receipt's prefix closure is a source-progress fact at that actual receipt,
not an issued-header or value-availability premise. Its correspondence to the
current selected visit must be established separately when route observations
use different full paths. No production state or scheduling rule is added. -/
theorem a_measured_last_receipt_interval_excludes_a_distinct_handoff
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {target transfer receipt : DecodedActorRow} {targetBinding loan : Binding}
    {key : EndpointKey} {issuance : List Issuance} {run : Elaboration} {index : Nat}
    {targetSymbolic previous : SymbolicBinding}
    {earlierRows laterRows : List (Nat × SymbolicBinding)}
    {first cut final : ActorProgressObservation} {firstCount cutCount finalCount : Nat}
    {prior later : List ActorCommitMutationRecord}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (targetMember : target ∈ data.activeRows path)
    (transferMember : transfer ∈ data.activeRows path)
    (receiptMember : receipt ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (transferred : loan.message.payload = .endpoint key)
    (used : targetBinding.message.sender = key ∨ targetBinding.message.receiver = key ∨
      targetBinding.message.payload = .endpoint key)
    (sending : transfer.actionId.side = .send) (receiving : receipt.actionId.side = .receive)
    (different : transfer.actionId.event ≠ target.actionId.event)
    (elaborated : elaborate data.source.resources (initialOwners data.source.resources)
      path.selections data.source 0 = some run)
    (sourceTarget : run.bindings[index]? = some (target.actionId.event, targetSymbolic))
    (targetInstantiated : instantiateBinding issuance targetSymbolic = some targetBinding)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some path.bindings)
    (shape : run.bindings.take index =
      earlierRows ++ (receipt.actionId.event, previous) :: laterRows)
    (previousPayload : previous.message.payload = .endpoint key)
    (untouched : ∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key)
    (receiptDone : first.doneState.done receipt.eventId = true)
    (receiptClosed : first.doneState.PrefixClosed (data.sourcePredecessors path) [receipt.eventId])
    (prefixChecked : actorCommitMutationHistoryCheck image choreo session
      first firstCount cut cutCount prior = true)
    (suffixChecked : actorCommitMutationHistoryCheck image choreo session
      cut cutCount final finalCount later = true)
    (pending : final.doneState.done target.eventId = false) :
    sourceRowReady data path cut transfer = false := by
  obtain ⟨sent, sentMember, sentAt⟩ := instantiated_path_binding_has_source_input mapped loanAt
  have sentMessage := (instantiated_binding_retains_message_and_actor_references sentAt).1
  have targetMessage := (instantiated_binding_retains_message_and_actor_references targetInstantiated).1
  have order := a_resource_handoff_is_outside_its_last_receipt_to_target_interval
    elaborated sourceTarget shape previousPayload untouched sentMember
    (sentMessage ▸ transferred) (targetMessage ▸ used)
  have noEarlier :
      (data.source.before transfer.actionId.event receipt.actionId.event = true ∨
        transfer.actionId.event = receipt.actionId.event) →
      sourceRowReady data path cut transfer = false := by
    intro past
    have done := a_handoff_at_or_before_a_completed_receipt_is_complete checked decoded selected
      transferMember receiptMember sending receiving past receiptDone receiptClosed
    have retained := (a_measured_history_preserves_completed_events prefixChecked done).1
    simp [sourceRowReady, retained]
  rcases order with before | same | same | after
  · exact noEarlier (.inl before)
  · exact noEarlier (.inr same)
  · exact False.elim (different same)
  · have targetAction := active_decoded_row_has_its_exact_source_action checked decoded selected targetMember
    have transferAction := active_decoded_row_has_its_exact_source_action checked decoded selected transferMember
    have parent := actor_precedence_is_a_source_predecessor targetMember transferMember
      (source_order_gives_actor_precedence targetAction transferAction after)
    have cutPending := (a_pending_final_event_was_pending_at_every_measured_commit suffixChecked pending).1
    exact a_pending_source_predecessor_blocks_its_successor parent cutPending

end Hibana
