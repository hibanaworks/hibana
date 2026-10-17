import Hibana.CarrierProfile

namespace Hibana.EndpointTransfer

/-- Proof-only observations for one physical binding. `usable` contains values
available to application code; a send future's private payload is not usable.
These lists are an abstraction of Rust values and transport receipts, not a
runtime owner table or a second progress cursor. -/
structure Observation (View Frame : Type) where
  usable : List View
  preparing : List View
  acceptedFutures : List View
  inFlight : List Frame

def Observation.authorityCount (state : Observation View Frame) : Nat :=
  state.usable.length + state.preparing.length + state.inFlight.length

/-- One binding's issuance is supplied by the durable initial-binding proof.
There is no transition that issues a second initial value. -/
def initial (view : View) : Observation View Frame :=
  { usable := [view], preparing := [], acceptedFutures := [], inFlight := [] }

inductive Action where
  | seal
  | accept
  | receive
  | retireAcceptedFuture
  | abandonPreparation
  | dropValue
  | fenceFrame
  | unchanged

/-- Boundaries of the owned send/receive implementation. Sealing removes the
application value before any poll. Transport acceptance moves its authority
to one affine receipt, retaining only the sender future's private witness.
Receive can publish a value before the sender retires that witness. Failure
can discard authority, but no rule restores an uncertain send to its owner.

To apply this model, an execution must supply authentic, affine, run-isolated
receipts and the implementation-to-observation correspondence. Those premises
are not established merely by constructing this proof model. -/
inductive Step : Action → Observation View Frame → Observation View Frame → Prop where
  | seal (state : Observation View Frame) (before after : List View)
      (view : View) :
      Step .seal { state with usable := before ++ view :: after }
        { state with
          usable := before ++ after
          preparing := view :: state.preparing }
  | accept (state : Observation View Frame) (before after : List View)
      (view : View) (frame : Frame) :
      Step .accept { state with preparing := before ++ view :: after }
        { state with
          preparing := before ++ after
          acceptedFutures := view :: state.acceptedFutures
          inFlight := state.inFlight ++ [frame] }
  | receive (state : Observation View Frame) (rest : List Frame)
      (frame : Frame) (fresh : View) :
      Step .receive { state with inFlight := frame :: rest }
        { state with inFlight := rest, usable := fresh :: state.usable }
  | retireAcceptedFuture (state : Observation View Frame)
      (before after : List View) (witness : View) :
      Step .retireAcceptedFuture
        { state with acceptedFutures := before ++ witness :: after }
        { state with acceptedFutures := before ++ after }
  | abandonPreparation (state : Observation View Frame)
      (before after : List View) (witness : View) :
      Step .abandonPreparation
        { state with preparing := before ++ witness :: after }
        { state with preparing := before ++ after }
  | dropValue (state : Observation View Frame) (before after : List View) (view : View) :
      Step .dropValue { state with usable := before ++ view :: after }
        { state with usable := before ++ after }
  | fenceFrame (state : Observation View Frame) (before after : List Frame) (frame : Frame) :
      Step .fenceFrame { state with inFlight := before ++ frame :: after }
        { state with inFlight := before ++ after }
  | unchanged (state : Observation View Frame) : Step .unchanged state state

inductive Execution : Observation View Frame → Observation View Frame → Prop where
  | refl (state) : Execution state state
  | tail : Execution first middle → Step action middle last → Execution first last

/-- Interpret a terminally committed receipt in the proof observations. The
low-level transport state retains its post-receipt cursor only on commit or
discard. The carrier theorem below requires actual commit resolution before
using this function to publish a value. A mere offer or requeue cannot do so.
The expected receipt includes channel, carrier generation and FIFO sequence. -/
def observeReceive?
    (state : Observation View TransportFrame) (transport : TransportState) (fresh : View) :
    Option (Observation View TransportFrame × TransportState) :=
  match transport.pollReceive, state.inFlight with
  | .delivered frame next, expected :: rest =>
      if frame = expected then
        some ({ state with inFlight := rest, usable := fresh :: state.usable }, next)
      else none
  | _, _ => none

end Hibana.EndpointTransfer

namespace Hibana

open EndpointTransfer

theorem EndpointTransfer.transfer_step_cannot_duplicate_authority
    (step : Step action before after) : after.authorityCount ≤ before.authorityCount := by
  cases step <;> simp only [Observation.authorityCount, List.length_append,
    List.length_cons, List.length_nil] <;> omega

theorem EndpointTransfer.transfer_execution_cannot_duplicate_authority
    (execution : Execution before after) : after.authorityCount ≤ before.authorityCount := by
  induction execution with
  | refl => exact Nat.le_refl _
  | tail _ step ih => exact Nat.le_trans (transfer_step_cannot_duplicate_authority step) ih

theorem EndpointTransfer.one_binding_has_at_most_one_transferable_authority
    (execution : Execution (initial view : Observation View Frame) final) :
    final.authorityCount ≤ 1 := by
  have bound := transfer_execution_cannot_duplicate_authority execution
  simpa [initial, Observation.authorityCount] using bound

theorem EndpointTransfer.one_binding_cannot_have_two_application_values
    (execution : Execution (initial view : Observation View Frame) final) :
    final.usable.length ≤ 1 := by
  have bound := one_binding_has_at_most_one_transferable_authority execution
  unfold Observation.authorityCount at bound
  omega

theorem EndpointTransfer.application_value_and_in_flight_receipt_are_exclusive
    (execution : Execution (initial view : Observation View Frame) final)
    (available : final.usable ≠ []) : final.inFlight = [] := by
  have bound := one_binding_has_at_most_one_transferable_authority execution
  unfold Observation.authorityCount at bound
  have nonempty : 0 < final.usable.length := List.length_pos_iff.mpr available
  have empty : final.inFlight.length = 0 := by omega
  exact List.length_eq_zero_iff.mp empty

theorem EndpointTransfer.accepted_future_retirement_cannot_restore_an_application_value
    (step : Step .retireAcceptedFuture before after) :
    after.usable = before.usable ∧ after.preparing = before.preparing ∧
      after.inFlight = before.inFlight := by
  cases step
  exact ⟨rfl, rfl, rfl⟩

theorem EndpointTransfer.abandoning_preparation_does_not_restore_a_value
    (step : Step .abandonPreparation before after) : after.usable = before.usable := by
  cases step
  rfl

/-- A receiver may use the value while the old sender future still exists.
Counting private future payloads as application values would reject this
legal schedule; waiting for sender publication would unnecessarily order it. -/
theorem EndpointTransfer.receive_does_not_require_sender_publication
    (sender receiver : View) (frame : Frame) :
    ∃ state : Observation View Frame,
      Execution (initial sender) state ∧ state.usable = [receiver] ∧
        state.acceptedFutures = [sender] ∧ state.inFlight = [] := by
  let sealed : Observation View Frame := {
    usable := [], preparing := [sender], acceptedFutures := [], inFlight := [] }
  let sent : Observation View Frame := {
    usable := [], preparing := [], acceptedFutures := [sender], inFlight := [frame] }
  let received : Observation View Frame := {
    usable := [receiver], preparing := [], acceptedFutures := [sender], inFlight := [] }
  have sealing : Step .seal (initial sender) sealed :=
    .seal (initial sender) [] [] sender
  have acceptance : Step .accept sealed sent := .accept sealed [] [] sender frame
  have reception : Step .receive sent received := .receive sent [] frame receiver
  exact ⟨received, .tail (.tail (.tail (.refl _) sealing) acceptance) reception,
    rfl, rfl, rfl⟩

theorem EndpointTransfer.observed_receipt_realizes_an_affine_transfer
    {state next : Observation View TransportFrame} {transport delivered : TransportState}
    (fresh : View) (observed : observeReceive? state transport fresh = some (next, delivered)) :
    Step .receive state next ∧
      ∃ frame, transport.pollReceive = .delivered frame delivered := by
  cases receipt : transport.pollReceive with
  | pending => simp [observeReceive?, receipt] at observed
  | peerClosed => simp [observeReceive?, receipt] at observed
  | delivered frame after =>
      cases queued : state.inFlight with
      | nil => simp [observeReceive?, receipt, queued] at observed
      | cons expected rest =>
          by_cases same : frame = expected
          · have pair :
                ({ state with inFlight := rest, usable := fresh :: state.usable }, after) =
                  (next, delivered) :=
              Option.some.inj (by simpa [observeReceive?, receipt, queued, same] using observed)
            have nextEq := congrArg Prod.fst pair
            have deliveredEq := congrArg Prod.snd pair
            dsimp only at nextEq deliveredEq
            subst next
            subst delivered
            have input : { state with inFlight := expected :: rest } = state := by
              cases state
              simp_all
            have step := Step.receive state rest expected fresh
            rw [input] at step
            exact ⟨step, frame, rfl⟩
          · simp [observeReceive?, receipt, queued, same] at observed

theorem EndpointTransfer.observed_affine_receipt_preserves_unique_authority
    {state next : Observation View TransportFrame} {transport delivered : TransportState}
    (initialView fresh : View)
    (execution : Execution (initial initialView) state)
    (wellFormed : transport.WellFormed)
    (observed : observeReceive? state transport fresh = some (next, delivered)) :
    next.usable.length ≤ 1 ∧ transport.delivered < delivered.delivered ∧
      ∃ frame, transport.pollReceive = .delivered frame delivered ∧
        frame.channel = transport.channel ∧ frame.sequence = transport.delivered := by
  obtain ⟨step, frame, received⟩ := observed_receipt_realizes_an_affine_transfer fresh observed
  have present := (transport_receive_is_fifo received).1
  have identity := wellFormed.2 transport.delivered frame present
  exact ⟨one_binding_cannot_have_two_application_values (.tail execution step),
    transport_receive_advances_affine_cursor received, frame, received, identity⟩

/-- Acceptance creates one fresh receipt while retaining the private sender
value. The frame is created at acceptance, not predicted when the future is
constructed. Its channel and sequence come from the existing carrier model. -/
theorem EndpointTransfer.ordered_carrier_send_preserves_unique_authority
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current accepted : carrier.State} {before : TransportState}
    {state : Observation View TransportFrame} {left right : List View}
    (initialView sending : View) (label : Fin 256)
    (execution : Execution (initial initialView) state)
    (preparing : state.preparing = left ++ sending :: right)
    (currentExact : abstraction current = .idle before)
    (sent : carrier.send current label = some accepted) :
    ∃ next frame after, Step .accept state next ∧ next.authorityCount ≤ 1 ∧
      abstraction accepted = .idle after ∧ after.frames = before.frames ++ [frame] ∧
      frame.channel = before.channel ∧ frame.sequence = before.frames.length ∧
      frame.frameLabel = label ∧ next.inFlight = state.inFlight ++ [frame] := by
  have exactSend := profile.2.2.sendExact current label
  rw [sent, currentExact] at exactSend
  cases result : before.send label with
  | none => simp [TransportBoundaryState.send, result] at exactSend
  | some after =>
      have mapped : some (abstraction accepted) = some (.idle after) := by
        simpa [TransportBoundaryState.send, result] using exactSend
      have acceptedExact := Option.some.inj mapped
      unfold TransportState.send at result
      split at result
      next closed => simp at result
      next allowed =>
        let frame : TransportFrame := {
          channel := before.channel, sequence := before.frames.length, frameLabel := label }
        let next : Observation View TransportFrame := { state with
          preparing := left ++ right
          acceptedFutures := sending :: state.acceptedFutures
          inFlight := state.inFlight ++ [frame] }
        have afterEq : { before with frames := before.frames ++ [frame] } = after :=
          Option.some.inj result
        have input : { state with preparing := left ++ sending :: right } = state := by
          cases state
          simp_all
        have step := Step.accept state left right sending frame
        rw [input] at step
        have bound := one_binding_has_at_most_one_transferable_authority (.tail execution step)
        refine ⟨next, frame, after, step, bound, acceptedExact, ?_, rfl, rfl, rfl, rfl⟩
        rw [← afterEq]

/-- Connect an actual carrier receipt to the same authority conservation
argument. Exact peer/run identity and affine delivery are supplied by the
existing ordered carrier refinement, not a new runtime ownership service.
`resolve .commit` is the proof boundary's terminal receipt disposition; it
does not introduce a Rust Transport method or a public Endpoint operation. -/
theorem EndpointTransfer.ordered_carrier_commit_preserves_unique_authority
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered committed : carrier.State} {frame : TransportFrame}
    {before : TransportState}
    {state : Observation View TransportFrame} {rest : List TransportFrame}
    (initialView fresh : View)
    (execution : Execution (initial initialView) state)
    (currentExact : abstraction current = .idle before)
    (wellFormed : before.WellFormed)
    (received : carrier.pollReceive current = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (queued : state.inFlight = frame :: rest) :
    ∃ next after, Step .receive state next ∧ next.usable.length ≤ 1 ∧
      next.usable = fresh :: state.usable ∧ next.inFlight = rest ∧
      abstraction committed = .idle after ∧ before.delivered < after.delivered ∧
      frame.channel = before.channel ∧ frame.sequence = before.delivered := by
  have exactPoll := profile.2.1.pollExact current
  rw [received] at exactPoll
  have abstractReceipt : (abstraction current).pollReceive =
      .delivered frame (abstraction offered) := by
    simpa [CarrierPoll.map] using exactPoll.symm
  obtain ⟨actualBefore, after, sourceExact, transportReceipt, offeredExact⟩ :=
    boundary_poll_issues_exact_receipt abstractReceipt
  have beforeEq : actualBefore = before := by
    have same := sourceExact.symm.trans currentExact
    exact TransportBoundaryState.idle.inj same
  subst actualBefore
  have exactResolve := profile.2.2.resolveExact offered .commit
  rw [resolved] at exactResolve
  have committedExact : abstraction committed = .idle after := by
    have same : some (abstraction committed) = some (.idle after) := by
      simpa [offeredExact, TransportBoundaryState.resolve] using exactResolve
    exact Option.some.inj same
  let next : Observation View TransportFrame :=
    { state with inFlight := rest, usable := fresh :: state.usable }
  have observed : observeReceive? state before fresh = some (next, after) := by
    simp [observeReceive?, transportReceipt, queued, next]
  have bound := (observed_affine_receipt_preserves_unique_authority
    initialView fresh execution wellFormed observed).1
  have step := (observed_receipt_realizes_an_affine_transfer fresh observed).1
  have identity := wellFormed.2 before.delivered frame
    (transport_receive_is_fifo transportReceipt).1
  exact ⟨next, after, step, bound, rfl, rfl, committedExact,
    transport_receive_advances_affine_cursor transportReceipt, identity⟩

end Hibana
