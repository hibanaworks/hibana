import Hibana.OwnedCommit
import Hibana.OwnedRetirement

namespace Hibana
open EndpointTransfer

/-- The same affine value is in application code or a private send future.
Completed sender witnesses are deliberately unconstrained. This predicate
interprets the existing transfer observations; it adds no runtime state. -/
def TransferValueLocated (value : View) (state : Observation View Frame) : Prop :=
  (state.usable = [value] ∧ state.preparing = [] ∧ state.inFlight = []) ∨
  (state.usable = [] ∧ state.preparing = [value] ∧ state.inFlight = [])

/-- The accepted receipt is the only authority, while any number of completed
sender witnesses can still exist. The identity is the carrier's actual frame. -/
def TransferFrameLocated (frame : Frame) (state : Observation View Frame) : Prop :=
  state.usable = [] ∧ state.preparing = [] ∧ state.inFlight = [frame]

private theorem a_singleton_cannot_hide_another_value
    {α : Type} {left right : List α} {value expected : α}
    (same : left ++ value :: right = [expected]) :
    left = [] ∧ value = expected ∧ right = [] := by
  have size := congrArg List.length same
  simp only [List.length_append, List.length_cons, List.length_nil] at size
  have emptyLeft : left = [] := List.length_eq_zero_iff.mp (by omega)
  have emptyRight : right = [] := List.length_eq_zero_iff.mp (by omega)
  subst left
  subst right
  exact ⟨rfl, by simpa using same, rfl⟩

theorem one_usable_authority_is_the_exact_value
    {value : View} {state : Observation View Frame}
    (bound : state.authorityCount ≤ 1) (member : value ∈ state.usable) :
    state.usable = [value] ∧ state.preparing = [] ∧ state.inFlight = [] := by
  have positive : 0 < state.usable.length := List.length_pos_iff.mpr (by
    intro empty
    simp [empty] at member)
  have lengths : state.usable.length = 1 ∧ state.preparing.length = 0 ∧
      state.inFlight.length = 0 := by
    simp only [Observation.authorityCount] at bound
    exact ⟨by omega, by omega, by omega⟩
  obtain ⟨only, singleton⟩ := List.length_eq_one_iff.mp lengths.1
  have same : value = only := by simpa [singleton] using member
  exact ⟨by simpa [same] using singleton,
    List.length_eq_zero_iff.mp lengths.2.1, List.length_eq_zero_iff.mp lengths.2.2⟩

/-- No ordinary computation, communication, pending poll or completed sender
retirement can change this value's location. Only acceptance of its next
handoff or its explicit retirement crosses this boundary. -/
theorem transfer_step_keeps_the_value_or_crosses_its_boundary
    {value : View} {before after : Observation View Frame} {action : Action}
    (step : Step action before after) (located : TransferValueLocated value before) :
    TransferValueLocated value after ∨
      action = .accept ∨ action = .abandonPreparation ∨ action = .dropValue := by
  cases step with
  | «seal» state left right moved =>
      rcases located with ⟨usable, preparing, frames⟩ | ⟨usable, _, _⟩
      · obtain ⟨rfl, rfl, rfl⟩ := a_singleton_cannot_hide_another_value usable
        exact .inl (.inr ⟨rfl, by simpa using preparing, frames⟩)
      · simp at usable
  | accept => exact .inr (.inl rfl)
  | receive state rest frame fresh =>
      rcases located with ⟨_, _, frames⟩ | ⟨_, _, frames⟩ <;> simp at frames
  | retireAcceptedFuture => exact .inl located
  | abandonPreparation => exact .inr (.inr (.inl rfl))
  | dropValue => exact .inr (.inr (.inr rfl))
  | fenceFrame state left right frame =>
      rcases located with ⟨_, _, frames⟩ | ⟨_, _, frames⟩ <;> simp at frames
  | unchanged => exact .inl located

/-- For every finite suffix after a checked receipt, preserve the exact
received value, or exhibit an actual subsequent handoff/retirement boundary.
The statement does not postulate that a source-ready operation is realizable. -/
theorem transfer_execution_keeps_the_value_until_handoff_or_retirement
    {value : View} {before after : Observation View Frame}
    (execution : Execution before after) (located : TransferValueLocated value before) :
    TransferValueLocated value after ∨
      ∃ prior next action, Execution before prior ∧ Step action prior next ∧
        TransferValueLocated value prior ∧
        (action = .accept ∨ action = .abandonPreparation ∨ action = .dropValue) ∧
        Execution next after := by
  induction execution with
  | refl => exact .inl located
  | tail history step ih =>
      rcases ih with still | ⟨prior, next, action, earlier, boundary, present, kind, suffix⟩
      · rcases transfer_step_keeps_the_value_or_crosses_its_boundary step still with kept | crossed
        · exact .inl kept
        · exact .inr ⟨_, _, _, history, step, still, crossed, .refl _⟩
      · exact .inr ⟨prior, next, action, earlier, boundary, present, kind, .tail suffix step⟩

theorem one_queued_authority_is_the_exact_frame
    {frame : Frame} {state : Observation View Frame}
    (bound : state.authorityCount ≤ 1) (member : frame ∈ state.inFlight) :
    TransferFrameLocated frame state := by
  have positive : 0 < state.inFlight.length := List.length_pos_iff.mpr (by
    intro empty
    simp [empty] at member)
  have lengths : state.usable.length = 0 ∧ state.preparing.length = 0 ∧
      state.inFlight.length = 1 := by
    simp only [Observation.authorityCount] at bound
    exact ⟨by omega, by omega, by omega⟩
  obtain ⟨only, singleton⟩ := List.length_eq_one_iff.mp lengths.2.2
  have same : frame = only := by simpa [singleton] using member
  exact ⟨List.length_eq_zero_iff.mp lengths.1,
    List.length_eq_zero_iff.mp lengths.2.1, by simpa [same] using singleton⟩

theorem transfer_step_keeps_the_frame_or_consumes_it
    {frame : Frame} {before after : Observation View Frame} {action : Action}
    (step : Step action before after) (located : TransferFrameLocated frame before) :
    TransferFrameLocated frame after ∨ action = .receive ∨ action = .fenceFrame := by
  cases step with
  | «seal» state left right moved => simp [TransferFrameLocated] at located
  | accept state left right moved sent => simp [TransferFrameLocated] at located
  | receive => exact .inr (.inl rfl)
  | retireAcceptedFuture => exact .inl located
  | abandonPreparation state left right moved => simp [TransferFrameLocated] at located
  | dropValue state left right moved => simp [TransferFrameLocated] at located
  | fenceFrame => exact .inr (.inr rfl)
  | unchanged => exact .inl located

theorem transfer_execution_keeps_the_frame_until_receive_or_fence
    {frame : Frame} {before after : Observation View Frame}
    (execution : Execution before after) (located : TransferFrameLocated frame before) :
    TransferFrameLocated frame after ∨
      ∃ prior next action, Execution before prior ∧ Step action prior next ∧
        TransferFrameLocated frame prior ∧ (action = .receive ∨ action = .fenceFrame) ∧
        Execution next after := by
  induction execution with
  | refl => exact .inl located
  | tail history step ih =>
      rcases ih with still | ⟨prior, next, action, earlier, boundary, present, kind, suffix⟩
      · rcases transfer_step_keeps_the_frame_or_consumes_it step still with kept | crossed
        · exact .inl kept
        · exact .inr ⟨_, _, _, history, step, still, crossed, .refl _⟩
      · exact .inr ⟨prior, next, action, earlier, boundary, present, kind, .tail suffix step⟩

theorem checked_actor_commit_preserves_actor_and_run
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    sample.after.actor = sample.before.actor ∧ sample.after.run = sample.before.run := by
  have body := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).2
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [dataAt] at body
  | some data =>
      cases pathAt : data.normalized.paths[sample.pathIndex]? with
      | none => simp [dataAt, pathAt] at body
      | some path =>
          cases rowAt : data.rows[sample.event]? with
          | none => simp [dataAt, pathAt, rowAt] at body
          | some row =>
              simp only [dataAt, pathAt, rowAt, Bool.and_eq_true] at body
              exact ⟨beq_iff_eq.mp body.1.1.1.2, beq_iff_eq.mp body.1.1.2⟩

theorem checked_owned_receive_publishes_its_projected_actor
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (receiving : facts.side = .receive) :
    sample.after.actor = facts.binding.receiverActor := by
  obtain ⟨_, actual, found, fields⟩ := accepted_owned_commit_binds_source_view_and_wire checked
  have same := Option.some.inj (found.symm.trans decoded)
  subst actual
  obtain ⟨_, _, _, actor, _⟩ := fields
  have current := (checked_actor_commit_preserves_actor_and_run
    (Bool.and_eq_true_iff.mp checked).1).1
  simp only [OwnedCommitFacts.actor?, OwnedCommitFacts.sourceFacts, OwnedSourceFacts.actor?, receiving, Option.some.injEq] at actor
  exact current.trans actor.symm

/-- At an actual source-checked send, the private value belongs to the
projected sender and acceptance creates the carrier's exact frame. Every
finite suffix retains that frame until a receive or explicit carrier fence.
The channel/label observation and copied payload are distinct evidence. -/
theorem source_checked_send_keeps_the_exact_frame_until_receive_or_fence
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (sendingSide : facts.side = .send)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current accepted : carrier.State} {before : TransportState}
    {state : Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {left right : List (Nat × Nat × Nat × Nat)}
    (initialView sending : Nat × Nat × Nat × Nat) (label : Fin 256)
    (held : (observedActorView? sample.before facts.resource.role).map
      (fun view => (sample.before.actor, view.header)) = some sending)
    (execution : Execution (initial initialView) state)
    (preparing : state.preparing = left ++ sending :: right)
    (currentExact : abstraction current = .idle before)
    (sent : carrier.send current label = some accepted)
    (identity : wire.identifies {
      channel := before.channel, sequence := before.frames.length, frameLabel := label }) :
    sending.1 = facts.binding.senderActor ∧ sample.SourceOrdered image choreo ∧
      wire.payload = facts.envelope ∧
      ∃ next frame after, Step .accept state next ∧ TransferFrameLocated frame next ∧
        abstraction accepted = .idle after ∧ wire.identifies frame ∧
        ∀ final, Execution next final → TransferFrameLocated frame final ∨
          ∃ prior following action, Execution next prior ∧ Step action prior following ∧
            TransferFrameLocated frame prior ∧ (action = .receive ∨ action = .fenceFrame) ∧
            Execution following final := by
  obtain ⟨ordered, actual, found, fields⟩ := accepted_owned_commit_binds_source_view_and_wire checked
  have same := Option.some.inj (found.symm.trans decoded)
  subst actual
  obtain ⟨_, _, _, actor, _, _, _, _, _, envelope⟩ := fields
  have sendingActor : sending.1 = sample.before.actor := by
    cases headerAt : observedActorView? sample.before facts.resource.role with
    | none => simp [headerAt] at held
    | some header =>
        simp only [headerAt, Option.map_some, Option.some.injEq] at held
        exact (congrArg Prod.fst held).symm
  simp only [OwnedCommitFacts.actor?, OwnedCommitFacts.sourceFacts, OwnedSourceFacts.actor?, sendingSide, Option.some.injEq] at actor
  obtain ⟨next, frame, after, step, bound, acceptedAt, _, channelAt, sequenceAt, labelAt, queued⟩ :=
    ordered_carrier_send_preserves_unique_authority profile initialView sending label
      execution preparing currentExact sent
  have member : frame ∈ next.inFlight := by
    rw [queued]
    exact List.mem_append.mpr (.inr List.mem_cons_self)
  have located := one_queued_authority_is_the_exact_frame bound member
  have frameExact : frame = {
      channel := before.channel, sequence := before.frames.length, frameLabel := label } := by
    cases frame
    simp_all
  refine ⟨sendingActor.trans actor.symm, ordered, envelope,
    next, frame, after, step, located, acceptedAt, ?_, ?_⟩
  · rw [frameExact]
    exact identity
  · intro final suffix
    exact transfer_execution_keeps_the_frame_until_receive_or_fence suffix located

/-- A real source-checked ordered receipt publishes one exact value at the
projected receiver. Every finite subsequent transfer history either retains
that value in app code/private future, or has a concrete handoff/retirement
boundary while that same value was still present. Fresh visits, fault
observability and general source-ready admission remain separate obligations. -/
theorem source_checked_receipt_keeps_the_exact_value_until_its_next_boundary
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (receiving : facts.side = .receive)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered committed : carrier.State} {frame : TransportFrame}
    {before : TransportState}
    {state : Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {rest : List TransportFrame} (initialView : Nat × Nat × Nat × Nat)
    (execution : Execution (initial initialView) state)
    (currentExact : abstraction current = .idle before)
    (wellFormed : before.WellFormed)
    (received : carrier.pollReceive current = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (identity : wire.identifies frame)
    (queued : state.inFlight = frame :: rest) :
    sample.after.actor = facts.binding.receiverActor ∧ sample.SourceOrdered image choreo ∧
      ∃ next after, Step .receive state next ∧
        TransferValueLocated (sample.after.actor, facts.afterView.header) next ∧
        abstraction committed = .idle after ∧ before.delivered < after.delivered ∧
        ∀ final, Execution next final →
          TransferValueLocated (sample.after.actor, facts.afterView.header) final ∨
          ∃ prior following action, Execution next prior ∧ Step action prior following ∧
            TransferValueLocated (sample.after.actor, facts.afterView.header) prior ∧
            (action = .accept ∨ action = .abandonPreparation ∨ action = .dropValue) ∧
            Execution following final := by
  obtain ⟨ordered, _, _, next, after, step, _, issued, carrierAt, advances, _⟩ :=
    source_checked_carrier_receive_preserves_unique_authority checked decoded receiving profile
      initialView execution currentExact wellFormed received resolved identity queued
  have bound := one_binding_has_at_most_one_transferable_authority (.tail execution step)
  have member : (sample.after.actor, facts.afterView.header) ∈ next.usable := by
    rw [issued]
    exact List.mem_cons_self
  have located : TransferValueLocated (sample.after.actor, facts.afterView.header) next :=
    .inl (one_usable_authority_is_the_exact_value bound member)
  refine ⟨checked_owned_receive_publishes_its_projected_actor checked decoded receiving,
    ordered, next, after, step, located, carrierAt, advances, ?_⟩
  intro final suffix
  exact transfer_execution_keeps_the_value_until_handoff_or_retirement suffix located

end Hibana
