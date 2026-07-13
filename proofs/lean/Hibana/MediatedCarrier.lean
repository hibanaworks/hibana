import Hibana.CarrierRefinement

namespace Hibana

/-- Delivery-neutral safety contract for the carrier/runtime observation
boundary. It says nothing about ordering, loss, replay, peer authenticity, or
eventual delivery. Those properties belong to `AffineCarrierRefinement` when a
deployment claims global fidelity or progress.

The contract captures only what the message-erased endpoint runtime needs from
one borrowed receive observation: the poll owns one exclusive receipt, requeue
reoffers that exact observation, terminal resolution is one-shot, and abort
invalidates the outstanding receipt. -/
structure MediatedCarrierContract (carrier : CarrierSemantics) : Prop where
  pollOwnsExclusiveReceipt :
    ∀ {current offered : carrier.State} {frame : TransportFrame},
      carrier.pollReceive current = .delivered frame offered ->
      carrier.pollReceive offered = .unavailable
  requeueReoffersExactObservation :
    ∀ {current offered restored : carrier.State} {frame : TransportFrame},
      carrier.pollReceive current = .delivered frame offered ->
      carrier.resolve offered .requeue = some restored ->
      ∃ reoffered,
        carrier.pollReceive restored = .delivered frame reoffered
  resolutionIsAffine :
    ∀ {current next : carrier.State} {resolution : ReceiptResolution},
      carrier.resolve current resolution = some next ->
      ∀ later, carrier.resolve next later = none
  abortInvalidatesReceipt :
    ∀ {current offered : carrier.State} {frame : TransportFrame},
      carrier.pollReceive current = .delivered frame offered ->
      ∀ resolution,
        carrier.resolve (carrier.abortPeer offered) resolution = none

private theorem mapped_unavailable_is_unavailable
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {state : carrier.State}
    (mapped : (carrier.pollReceive state).map abstraction = .unavailable) :
    carrier.pollReceive state = .unavailable := by
  cases polled : carrier.pollReceive state with
  | unavailable => rfl
  | pending => simp [polled, CarrierPoll.map] at mapped
  | peerClosed => simp [polled, CarrierPoll.map] at mapped
  | delivered frame next => simp [polled, CarrierPoll.map] at mapped

private theorem mapped_delivery_has_exact_frame
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {state offered : carrier.State}
    {frame : TransportFrame}
    (mapped :
      (carrier.pollReceive state).map abstraction =
        .delivered frame (abstraction offered)) :
    ∃ reoffered,
      carrier.pollReceive state = .delivered frame reoffered := by
  cases polled : carrier.pollReceive state with
  | unavailable => simp [polled, CarrierPoll.map] at mapped
  | pending => simp [polled, CarrierPoll.map] at mapped
  | peerClosed => simp [polled, CarrierPoll.map] at mapped
  | delivered candidate next =>
      have exact :
          CarrierPoll.delivered candidate (abstraction next) =
            CarrierPoll.delivered frame (abstraction offered) := by
        simpa [polled, CarrierPoll.map] using mapped
      have frameExact : candidate = frame := by
        injection exact
      subst candidate
      exact ⟨next, rfl⟩

private theorem mapped_resolution_none_is_none
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {state : carrier.State}
    {resolution : ReceiptResolution}
    (mapped :
      (carrier.resolve state resolution).map abstraction = none) :
    carrier.resolve state resolution = none := by
  cases resolved : carrier.resolve state resolution with
  | none => rfl
  | some next => simp [resolved] at mapped

/-- Every strong affine-delivery refinement supplies delivery-neutral mediated
safety. The converse is intentionally absent: a lossy or reordering carrier can
satisfy receipt safety without pretending to be FIFO or no-replay. -/
theorem affine_carrier_refinement_establishes_mediated_contract
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    (refinement : AffineCarrierRefinement carrier abstraction) :
    MediatedCarrierContract carrier := by
  refine {
    pollOwnsExclusiveReceipt := ?_
    requeueReoffersExactObservation := ?_
    resolutionIsAffine := ?_
    abortInvalidatesReceipt := ?_
  }
  · intro current offered frame polled
    have abstractPoll := refined_carrier_poll_is_exact refinement polled
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt abstractPoll
    have mapped := refinement.pollExact offered
    rw [offeredExact] at mapped
    exact mapped_unavailable_is_unavailable mapped
  · intro current offered restored frame polled requeued
    have abstractPoll := refined_carrier_poll_is_exact refinement polled
    have restoredExact :=
      refined_carrier_requeue_restores_abstract_state refinement polled requeued
    have mapped := refinement.pollExact restored
    rw [restoredExact, abstractPoll] at mapped
    exact mapped_delivery_has_exact_frame mapped
  · intro current next resolution resolved
    exact refined_carrier_duplicate_resolution_rejected refinement resolved
  · intro current offered frame polled resolution
    have abstractPoll := refined_carrier_poll_is_exact refinement polled
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt abstractPoll
    have abortedExact := refinement.abortExact offered
    have resolvedExact := refinement.resolveExact (carrier.abortPeer offered) resolution
    rw [abortedExact, offeredExact] at resolvedExact
    exact mapped_resolution_none_is_none resolvedExact

theorem reference_carrier_satisfies_mediated_contract :
    MediatedCarrierContract referenceCarrierSemantics :=
  affine_carrier_refinement_establishes_mediated_contract
    reference_carrier_refines_affine_boundary

/-- Delivery-neutral witness that may lose every accepted send while retaining
the exact receive-receipt discipline. It is intentionally unsuitable for any
global delivery theorem. -/
def droppingCarrierSemantics : CarrierSemantics where
  State := TransportBoundaryState
  initial := TransportBoundaryState.initial
  send := fun state _ => some state
  closePeer := TransportBoundaryState.closePeer
  abortPeer := TransportBoundaryState.abortPeer
  pollReceive := TransportBoundaryState.pollReceive
  resolve := TransportBoundaryState.resolve

theorem dropping_carrier_satisfies_mediated_contract :
    MediatedCarrierContract droppingCarrierSemantics := by
  refine {
    pollOwnsExclusiveReceipt := ?_
    requeueReoffersExactObservation := ?_
    resolutionIsAffine := ?_
    abortInvalidatesReceipt := ?_
  }
  · intro current offered frame polled
    change current.pollReceive = .delivered frame offered at polled
    change offered.pollReceive = .unavailable
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt polled
    subst offered
    rfl
  · intro current offered restored frame polled requeued
    change current.pollReceive = .delivered frame offered at polled
    change offered.resolve .requeue = some restored at requeued
    change ∃ reoffered, restored.pollReceive = .delivered frame reoffered
    exact ⟨offered, requeued_receipt_reoffers_same_frame polled requeued⟩
  · intro current next resolution resolved
    change current.resolve resolution = some next at resolved
    change ∀ later, next.resolve later = none
    exact resolved_receipt_cannot_resolve_again resolved
  · intro current offered frame polled resolution
    change current.pollReceive = .delivered frame offered at polled
    change (offered.abortPeer).resolve resolution = none
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt polled
    subst offered
    rfl

/-- Mediated carrier safety is strictly weaker than affine delivery: a carrier
that may lose an accepted send cannot refine the strong reference semantics,
regardless of the chosen abstraction. -/
theorem dropping_carrier_has_no_affine_refinement :
    ¬ ∃ abstraction,
      AffineCarrierRefinement droppingCarrierSemantics abstraction := by
  rintro ⟨abstraction, refinement⟩
  let channel : TransportChannel := {
    session := 0
    generation := 0
    lane := 0
    sender := 0
    receiver := 1
  }
  have sent := refinement.sendExact
    (droppingCarrierSemantics.initial channel) (0 : Fin 256)
  have initialExact := refinement.initialExact channel
  change abstraction (TransportBoundaryState.initial channel) =
    TransportBoundaryState.initial channel at initialExact
  change some (abstraction (TransportBoundaryState.initial channel)) =
    (abstraction (TransportBoundaryState.initial channel)).send 0 at sent
  simp only [initialExact] at sent
  simp [TransportBoundaryState.initial, TransportBoundaryState.send,
    TransportState.initial, TransportState.send] at sent

end Hibana
