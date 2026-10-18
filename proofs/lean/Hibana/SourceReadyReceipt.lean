import Hibana.ActorCommitHistory
import Hibana.ResourceHeaderTransfer

namespace Hibana
open EndpointDelegation

/-- A wire-checked commit names the resource and side of its actual source
row, even when its recorded route path differs from the current selected path.
This derives identity from the immutable source, not from an issued-header
premise or an assumption that all route selections have already propagated. -/
theorem checked_owned_source_row_has_its_exact_resource_and_side
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {binding : Binding} {key : EndpointKey} {facts : OwnedCommitFacts}
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (payload : binding.message.payload = .endpoint key)
    (eventAt : sample.event = row.eventId)
    (factsAt : sample.ownedFacts? image choreo = some facts) :
    facts.resource = key ∧ facts.side = row.actionId.side := by
  obtain ⟨actualData, actualPath, actualRow, entry, dataAt, pathAt, rowAt,
    entryAt, actualPayload, sideAt, _, _⟩ :=
    decoded_owned_facts_retain_the_source_row_and_view factsAt
  have sameData := Option.some.inj (dataAt.symm.trans decoded)
  subst actualData
  have ownIndex := decoded_actor_row_is_at_its_own_index (decoded_actor_order_fields decoded).2.2.2.2.1
    (List.mem_filter.mp member).1
  have sameRow := Option.some.inj (rowAt.symm.trans (by simpa only [eventAt] using ownIndex))
  subst actualRow
  have entryEvent : entry.1 = row.actionId.event := beq_iff_eq.mp
    (List.find?_some (p := fun pair : Nat × Binding => pair.1 == row.actionId.event)
      (a := entry) entryAt)
  have present : (row.actionId.event, entry.2) ∈ actualPath.bindings := by
    have pair : entry = (row.actionId.event, entry.2) := Prod.ext entryEvent rfl
    rw [← pair]
    exact List.mem_of_find?_eq_some entryAt
  have messageSame := normalized_paths_agree_on_a_shared_source_message
    (decoded_actor_order_retains_its_normalized_source decoded).2 selected
    (List.mem_of_getElem? pathAt) (List.mem_of_find?_eq_some located) present
  have keys : Payload.endpoint key = .endpoint facts.resource := by
    rw [← payload, messageSame]
    exact actualPayload
  exact ⟨(Payload.endpoint.inj keys).symm, sideAt.symm⟩

/-- Proof-only output of the source/history bridge. The current final cut is
separate from the receipt boundary; no runtime data or authority is created. -/
def SourceReadyReceiptEvidence (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (data : DecodedActorOrder) (path : NormalizedPath) (row : DecodedActorRow)
    (binding : Binding) (key : EndpointKey) (records : List ActorCommitRecord)
    (final : ActorProgressObservation) : Prop :=
  ∃ (issuance : List Issuance) (run : Elaboration) (index : Nat) (symbolic : SymbolicBinding),
    elaborate data.source.resources (initialOwners data.source.resources)
      path.selections data.source 0 = some run ∧
    run.bindings[index]? = some (row.actionId.event, symbolic) ∧
    instantiateBinding issuance symbolic = some binding ∧
    run.equations.all (equationAccepted issuance) = true ∧
    (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some path.bindings ∧
    ((instantiate issuance ((initialOwners data.source.resources) key) = some data.actor ∧
        ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint key) ∨
      ∃ receipt loan earlier record later facts,
        receipt ∈ data.activeRows path ∧ receipt.actionId.side = .receive ∧
        path.bindings.find? (fun entry => entry.1 == receipt.actionId.event) =
          some (receipt.actionId.event, loan) ∧
        loan.message.payload = .endpoint key ∧ loan.receiverActor = data.actor ∧
        receipt.eventId ∈ data.sourcePredecessors path row.eventId ∧
        records = earlier ++ record :: later ∧ record.step.event = receipt.eventId ∧
        record.step.ownedCheck image choreo record.wire session = true ∧
        record.step.ownedFacts? image choreo = some facts ∧
        facts.resource = key ∧ facts.side = .receive ∧
        observedResourceTag? record.step.after key.role =
          some (record.step.after.actor, facts.afterView.header) ∧
        actorCommitHistoryCheck image choreo session record.step.after final later = true ∧
        (∀ next ∈ later, next.step.event ≠ receipt.eventId) ∧
        record.step.after.doneState.done receipt.eventId = true ∧
        (∀ next ∈ later, next.step.before.doneState.done receipt.eventId = true ∧
          next.step.after.doneState.done receipt.eventId = true) ∧
        final.doneState.done receipt.eventId = true ∧
        ∃ earlierRows previous laterRows,
          run.bindings.take index = earlierRows ++ (receipt.actionId.event, previous) :: laterRows ∧
          previous.message.payload = .endpoint key ∧
          (∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) ∧
          instantiateBinding issuance previous = some loan)

/-- Source readiness derives either the initial source issuer, or the actual
latest checked receipt for the required resource in this visit. The receipt's
wire, side, resource and published tag are recovered from the complete commit
history. No admission observation, issued-header bit, or live-value assumption
is an input. The baseline covers receipts for this resource only; independent
parallel work and already completed outside continuations need not reset.

The result supplies a real receipt boundary, not possession at the final cut.
Its subsequent transfer history, failure/closure, and fresh-visit correspondence
remain necessary to derive continued availability. Observation completeness and
the same-run correspondence remain implementation contracts. -/
theorem source_ready_resource_has_initial_issuer_or_current_wire_receipt
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {binding : Binding} {key : EndpointKey}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (required : match row.actionId.side with
      | .receive => binding.message.receiver = key
      | .send => binding.message.sender = key ∨ binding.message.payload = .endpoint key
      | .local => binding.message.sender = key)
    (ready : sourceRowReady data path final row = true)
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (baseline : ∀ receipt ∈ data.activeRows path, ∀ loan,
      receipt.actionId.side = .receive →
      path.bindings.find? (fun entry => entry.1 == receipt.actionId.event) =
        some (receipt.actionId.event, loan) →
      loan.message.payload = .endpoint key → first.doneState.done receipt.eventId = false) :
    SourceReadyReceiptEvidence image choreo session data path row binding key records final := by
  obtain ⟨issuance, run, index, symbolic, elaborated, sourceRow, instantiated,
    solved, mapped, origin⟩ :=
    source_ready_resource_row_has_initial_authority_or_a_completed_receipt
      checked decoded selected member located required ready
  refine ⟨issuance, run, index, symbolic, elaborated, sourceRow, instantiated, solved, mapped, ?_⟩
  rcases origin with initial | ⟨receipt, loan, receiptMember, receiving, loanAt,
    payload, actorAt, predecessor, completed, handoffPrefix⟩
  · exact .inl initial
  · obtain ⟨earlier, record, later, recordsAt, eventAt, wireChecked, _, suffix,
      lastRecord, afterDone, retained⟩ :=
      completed_owned_source_row_has_a_wire_checked_commit decoded selected receiptMember
        loanAt payload history (baseline receipt receiptMember loan receiving loanAt payload) completed
    obtain ⟨_, facts, factsAt, _⟩ := accepted_owned_commit_binds_source_view_and_wire wireChecked
    obtain ⟨resourceAt, sideAt⟩ := checked_owned_source_row_has_its_exact_resource_and_side
      decoded selected receiptMember loanAt payload eventAt factsAt
    have receiveAt : facts.side = .receive := sideAt.trans receiving
    have tagAt := (checked_owned_receive_publishes_its_exact_observed_tag wireChecked factsAt receiveAt).2
    rw [resourceAt] at tagAt
    exact .inr ⟨receipt, loan, earlier, record, later, facts,
      receiptMember, receiving, loanAt, payload, actorAt, predecessor, recordsAt, eventAt,
      wireChecked, factsAt, resourceAt, receiveAt, tagAt, suffix, lastRecord,
      afterDone, retained, completed, handoffPrefix⟩

end Hibana
