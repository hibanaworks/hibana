import Hibana.TransportContract

namespace Hibana

/-- The only terminal dispositions of one transport receive receipt. Commit and
discard both advance the carrier cursor; requeue restores the exact observation
point without committing protocol progress. -/
inductive ReceiptResolution where
  | commit
  | requeue
  | discard
  deriving Repr, DecidableEq

/-- Poll result for a carrier model whose state is explicit. `unavailable`
means that a previous borrowed frame still owns the one-shot receipt. Rust
prevents this poll statically; the model keeps the impossible call fail closed. -/
inductive CarrierPoll (State : Type) where
  | unavailable
  | pending
  | peerClosed
  | delivered (frame : TransportFrame) (next : State)

def CarrierPoll.map (mapState : Left -> Right) : CarrierPoll Left -> CarrierPoll Right
  | .unavailable => .unavailable
  | .pending => .pending
  | .peerClosed => .peerClosed
  | .delivered frame next => .delivered frame (mapState next)

/-- Reference state at the carrier/runtime ownership boundary. A delivered
frame is not silently committed: the outstanding receipt retains both the
pre-poll state used by requeue and the post-poll state used by commit/discard. -/
inductive TransportBoundaryState where
  | idle (transport : TransportState)
  | offered
      (before : TransportState)
      (frame : TransportFrame)
      (after : TransportState)
  deriving Repr, DecidableEq

def TransportBoundaryState.initial
    (channel : TransportChannel) : TransportBoundaryState :=
  .idle (.initial channel)

def TransportBoundaryState.send
    (state : TransportBoundaryState)
    (frameLabel : Fin 256) : Option TransportBoundaryState :=
  match state with
  | .idle transport => (transport.send frameLabel).map .idle
  | .offered _ _ _ => none

/-- Peer close is monotone even while a borrowed frame is outstanding. Closing
both snapshots makes close-then-requeue equal requeue-then-close. -/
def TransportBoundaryState.closePeer :
    TransportBoundaryState -> TransportBoundaryState
  | .idle transport => .idle transport.closePeer
  | .offered before frame after =>
      .offered before.closePeer frame after.closePeer

/-- Abort invalidates an outstanding receipt and retires its current transport
direction. No later requeue can revive the abandoned frame. -/
def TransportBoundaryState.abortPeer :
    TransportBoundaryState -> TransportBoundaryState
  | .idle transport => .idle transport.abortPeer
  | .offered _ _ after => .idle after.abortPeer

def TransportBoundaryState.pollReceive :
    TransportBoundaryState -> CarrierPoll TransportBoundaryState
  | .offered _ _ _ => .unavailable
  | .idle transport =>
      match transport.pollReceive with
      | .pending => .pending
      | .peerClosed => .peerClosed
      | .delivered frame next =>
          .delivered frame (.offered transport frame next)

def TransportBoundaryState.resolve
    (state : TransportBoundaryState)
    (resolution : ReceiptResolution) : Option TransportBoundaryState :=
  match state with
  | .idle _ => none
  | .offered before _ after =>
      match resolution with
      | .requeue => some (.idle before)
      | .commit | .discard => some (.idle after)

theorem boundary_poll_issues_exact_receipt
    {current next : TransportBoundaryState}
    {frame : TransportFrame}
    (polled : current.pollReceive = .delivered frame next) :
    ∃ before after,
      current = .idle before /\
      before.pollReceive = .delivered frame after /\
      next = .offered before frame after := by
  cases current with
  | offered before offeredFrame after =>
      simp [TransportBoundaryState.pollReceive] at polled
  | idle before =>
      cases transportPoll : before.pollReceive with
      | pending => simp [TransportBoundaryState.pollReceive, transportPoll] at polled
      | peerClosed => simp [TransportBoundaryState.pollReceive, transportPoll] at polled
      | delivered candidate after =>
          have exact :
              CarrierPoll.delivered candidate
                  (.offered before candidate after) =
                .delivered frame next := by
            simpa [TransportBoundaryState.pollReceive, transportPoll] using polled
          cases exact
          exact ⟨before, after, rfl, transportPoll, rfl⟩

theorem offered_receipt_blocks_second_poll
    (before after : TransportState) (frame : TransportFrame) :
    (TransportBoundaryState.offered before frame after).pollReceive =
      .unavailable := rfl

theorem receipt_resolution_is_exact
    (before after : TransportState)
    (frame : TransportFrame)
    (resolution : ReceiptResolution) :
    (TransportBoundaryState.offered before frame after).resolve resolution =
      some (.idle (match resolution with
        | .requeue => before
        | .commit | .discard => after)) := by
  cases resolution <;> rfl

theorem resolved_receipt_cannot_resolve_again
    {current next : TransportBoundaryState}
    {resolution : ReceiptResolution}
    (resolved : current.resolve resolution = some next) :
    ∀ later, next.resolve later = none := by
  cases current with
  | idle transport => simp [TransportBoundaryState.resolve] at resolved
  | offered before frame after =>
      cases resolution <;>
        simp [TransportBoundaryState.resolve] at resolved <;>
        subst next <;>
        intro later <;>
        rfl

theorem polled_receipt_requeue_restores_exact_state
    {current offered restored : TransportBoundaryState}
    {frame : TransportFrame}
    (polled : current.pollReceive = .delivered frame offered)
    (requeued : offered.resolve .requeue = some restored) :
    restored = current := by
  obtain ⟨before, after, currentExact, transportPoll, offeredExact⟩ :=
    boundary_poll_issues_exact_receipt polled
  subst current
  subst offered
  exact (Option.some.inj (by
    simpa [TransportBoundaryState.resolve] using requeued)).symm

theorem requeued_receipt_reoffers_same_frame
    {current offered restored : TransportBoundaryState}
    {frame : TransportFrame}
    (polled : current.pollReceive = .delivered frame offered)
    (requeued : offered.resolve .requeue = some restored) :
    restored.pollReceive = .delivered frame offered := by
  rw [polled_receipt_requeue_restores_exact_state polled requeued]
  exact polled

theorem committed_receipt_advances_affine_cursor
    {current offered committed : TransportBoundaryState}
    {frame : TransportFrame}
    (polled : current.pollReceive = .delivered frame offered)
    (resolved : offered.resolve .commit = some committed) :
    ∃ before after,
      current = .idle before /\
      committed = .idle after /\
      before.delivered < after.delivered := by
  obtain ⟨before, after, currentExact, transportPoll, offeredExact⟩ :=
    boundary_poll_issues_exact_receipt polled
  subst current
  subst offered
  have committedExact : committed = .idle after := by
    exact (Option.some.inj (by
      simpa [TransportBoundaryState.resolve] using resolved)).symm
  exact ⟨before, after, rfl, committedExact,
    transport_receive_advances_affine_cursor transportPoll⟩

theorem discarded_receipt_advances_affine_cursor
    {current offered discarded : TransportBoundaryState}
    {frame : TransportFrame}
    (polled : current.pollReceive = .delivered frame offered)
    (resolved : offered.resolve .discard = some discarded) :
    ∃ before after,
      current = .idle before /\
      discarded = .idle after /\
      before.delivered < after.delivered := by
  obtain ⟨before, after, currentExact, transportPoll, offeredExact⟩ :=
    boundary_poll_issues_exact_receipt polled
  subst current
  subst offered
  have discardedExact : discarded = .idle after := by
    exact (Option.some.inj (by
      simpa [TransportBoundaryState.resolve] using resolved)).symm
  exact ⟨before, after, rfl, discardedExact,
    transport_receive_advances_affine_cursor transportPoll⟩

theorem close_and_receipt_requeue_commute
    (before after : TransportState) (frame : TransportFrame) :
    ((TransportBoundaryState.offered before frame after).closePeer.resolve
        .requeue) =
      ((TransportBoundaryState.offered before frame after).resolve .requeue).map
        TransportBoundaryState.closePeer := rfl

theorem abort_invalidates_outstanding_receipt
    (before after : TransportState) (frame : TransportFrame)
    (resolution : ReceiptResolution) :
    ((TransportBoundaryState.offered before frame after).abortPeer.resolve
        resolution) = none := rfl

/-- Protocol-neutral semantics supplied by one concrete carrier model. The
carrier may implement any wire protocol; only these observable operations are
related to Hibana's reference boundary. -/
structure CarrierSemantics where
  State : Type
  initial : TransportChannel -> State
  send : State -> Fin 256 -> Option State
  closePeer : State -> State
  abortPeer : State -> State
  pollReceive : State -> CarrierPoll State
  resolve : State -> ReceiptResolution -> Option State

/-- A concrete carrier conforms only when every observable operation commutes
with one explicit abstraction into the receipt-aware reference model. This is a
deployment proof obligation; a protocol certificate cannot manufacture it. -/
structure AffineCarrierRefinement
    (carrier : CarrierSemantics)
    (abstraction : carrier.State -> TransportBoundaryState) : Prop where
  initialExact : ∀ channel,
    abstraction (carrier.initial channel) = .initial channel
  sendExact : ∀ state frameLabel,
    (carrier.send state frameLabel).map abstraction =
      (abstraction state).send frameLabel
  closeExact : ∀ state,
    abstraction (carrier.closePeer state) = (abstraction state).closePeer
  abortExact : ∀ state,
    abstraction (carrier.abortPeer state) = (abstraction state).abortPeer
  pollExact : ∀ state,
    (carrier.pollReceive state).map abstraction =
      (abstraction state).pollReceive
  resolveExact : ∀ state resolution,
    (carrier.resolve state resolution).map abstraction =
      (abstraction state).resolve resolution

def referenceCarrierSemantics : CarrierSemantics where
  State := TransportBoundaryState
  initial := TransportBoundaryState.initial
  send := TransportBoundaryState.send
  closePeer := TransportBoundaryState.closePeer
  abortPeer := TransportBoundaryState.abortPeer
  pollReceive := TransportBoundaryState.pollReceive
  resolve := TransportBoundaryState.resolve

theorem reference_carrier_refines_affine_boundary :
    AffineCarrierRefinement referenceCarrierSemantics id := by
  refine {
    initialExact := ?_
    sendExact := ?_
    closeExact := ?_
    abortExact := ?_
    pollExact := ?_
    resolveExact := ?_
  }
  · intro channel
    rfl
  · intro state frameLabel
    cases state <;> simp [referenceCarrierSemantics,
      TransportBoundaryState.send]
  · intro state
    rfl
  · intro state
    rfl
  · intro state
    cases polled : state.pollReceive <;>
      simp [referenceCarrierSemantics, CarrierPoll.map, polled]
  · intro state resolution
    cases state <;> cases resolution <;>
      simp [referenceCarrierSemantics, TransportBoundaryState.resolve]

theorem refined_carrier_poll_is_exact
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    (refinement : AffineCarrierRefinement carrier abstraction)
    {current next : carrier.State}
    {frame : TransportFrame}
    (polled : carrier.pollReceive current = .delivered frame next) :
    (abstraction current).pollReceive =
      CarrierPoll.delivered frame (abstraction next) := by
  have exact := refinement.pollExact current
  rw [polled] at exact
  simpa [CarrierPoll.map] using exact.symm

theorem refined_carrier_requeue_restores_abstract_state
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    (refinement : AffineCarrierRefinement carrier abstraction)
    {current offered restored : carrier.State}
    {frame : TransportFrame}
    (polled : carrier.pollReceive current = .delivered frame offered)
    (requeued : carrier.resolve offered .requeue = some restored) :
    abstraction restored = abstraction current := by
  have abstractPoll := refined_carrier_poll_is_exact refinement polled
  have exactResolve := refinement.resolveExact offered .requeue
  rw [requeued] at exactResolve
  have abstractRequeue :
      (abstraction offered).resolve .requeue =
        some (abstraction restored) := by
    simpa using exactResolve.symm
  exact polled_receipt_requeue_restores_exact_state
    abstractPoll abstractRequeue

theorem refined_carrier_duplicate_resolution_rejected
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    (refinement : AffineCarrierRefinement carrier abstraction)
    {current next : carrier.State}
    {resolution : ReceiptResolution}
    (resolved : carrier.resolve current resolution = some next) :
    ∀ later, carrier.resolve next later = none := by
  have exactResolve := refinement.resolveExact current resolution
  rw [resolved] at exactResolve
  have abstractResolved :
      (abstraction current).resolve resolution =
        some (abstraction next) := by
    simpa using exactResolve.symm
  have abstractNone := resolved_receipt_cannot_resolve_again abstractResolved
  intro later
  have exactLater := refinement.resolveExact next later
  rw [abstractNone later] at exactLater
  cases laterResolution : carrier.resolve next later with
  | none => rfl
  | some laterState => simp [laterResolution] at exactLater

end Hibana
