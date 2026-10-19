import Hibana.SourceTransferAvailability

namespace Hibana
open EndpointDelegation

/-- A source-ready owned send cannot be displaced by another accepted
handoff of that same binding. Unlike an ordinary use, its own acceptance is
permitted progress. This does not impose an index order on independent work. -/
theorem checked_ready_owned_send_and_acceptance_name_the_same_action
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer target : DecodedActorRow}
    {loan intended : Binding} {key : EndpointKey} {snapshot : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path)
    (targetMember : target ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (targetAt : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, intended))
    (transferred : loan.message.payload = .endpoint key)
    (intendedPayload : intended.message.payload = .endpoint key)
    (sending : transfer.actionId.side = .send) (targetSending : target.actionId.side = .send)
    (transferReady : sourceRowReady data path snapshot transfer = true)
    (targetReady : sourceRowReady data path snapshot target = true) :
    transfer.actionId = target.actionId ∧ loan = intended := by
  have same : transfer.actionId.event = target.actionId.event := by
    by_cases same : transfer.actionId.event = target.actionId.event
    · exact same
    · have ordered := accepted_resource_transfer_and_use_have_local_causal_order
        checked decoded selected transferMember targetMember loanAt targetAt transferred
        (Or.inr (Or.inr intendedPayload)) same
      have blocked := source_ready_use_excludes_a_distinct_resource_transfer ordered targetReady
      rw [blocked] at transferReady
      contradiction
  have located := loanAt
  rw [same] at located
  have bindingSame := congrArg Prod.snd (Option.some.inj (located.symm.trans targetAt))
  have first := congrArg (fun event => ActionId.mk event transfer.actionId.side) same
  have second := congrArg (ActionId.mk target.actionId.event) (sending.trans targetSending.symm)
  exact ⟨first.trans second, bindingSame⟩

/-- Copied observations at a source-ready handoff cut. Unlike a live-use
interval, a retirement is allowed to report its actual fault. Acceptance uses
the existing cursor/source checker; it does not require sender publication. -/
def SourceHandoffBoundaryEvidence
    (image : RustDescriptorImage) (choreo : Choreo) (data : DecodedActorOrder)
    (path : NormalizedPath) (key : EndpointKey) (target : DecodedActorRow)
    (action : EndpointTransfer.Action) : Prop :=
  match action with
  | .accept => SourceUseBoundaryEvidence image choreo data path key target action
  | .abandonPreparation | .dropValue =>
      ∃ sample : ViewRetirementObservation,
        sample.check image choreo = true ∧ sample.view = key.role ∧
        observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true ∧
        sample.before.doneState.done target.eventId = false
  | .seal | .receive | .retireAcceptedFuture | .fenceFrame | .unchanged => True

/-- An observed boundary either accepts this exact source action or records
a checked retirement fault. These are proof observations, not runtime state. -/
def SourceHandoffBoundaryOutcome
    (image : RustDescriptorImage) (choreo : Choreo) (data : DecodedActorOrder)
    (path : NormalizedPath) (key : EndpointKey) (target : DecodedActorRow)
    (action : EndpointTransfer.Action) : Prop :=
  (action = .accept ∧ ∃ sample : ActorAdmissionObservation, ∃ transfer : DecodedActorRow,
    sample.check image choreo = true ∧
    data.normalized.paths[sample.pathIndex]? = some path ∧ transfer ∈ data.activeRows path ∧
    transfer.actionId = target.actionId ∧ sample.admitted[transfer.eventId]? = some true) ∨
  ((action = .abandonPreparation ∨ action = .dropValue) ∧
    ∃ sample : ViewRetirementObservation,
      sample.check image choreo = true ∧ sample.view = key.role ∧ sample.after.fault ≠ none)

theorem checked_source_handoff_boundary_is_its_own_acceptance_or_a_fault
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {target : DecodedActorRow} {intended : Binding}
    {action : EndpointTransfer.Action}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, intended))
    (payload : intended.message.payload = .endpoint key)
    (sending : target.actionId.side = .send)
    (required : sourceActionRequiresView key.role intended target.actionId.side = true)
    (evidence : SourceHandoffBoundaryEvidence image choreo data path key target action)
    (kind : action = .accept ∨ action = .abandonPreparation ∨ action = .dropValue) :
    SourceHandoffBoundaryOutcome image choreo data path key target action := by
  rcases kind with rfl | rfl | rfl
  · obtain ⟨sample, transfer, loan, admitted, pathAt, transferMember, transferSending,
      loanAt, transferred, actual, ready⟩ := evidence
    obtain ⟨actualData, actualPath, actualAt, selectedAt, rows⟩ :=
      accepted_actor_admission_has_exact_source_readiness admitted
    have sameData := Option.some.inj (actualAt.symm.trans decoded)
    subst actualData
    have samePath := Option.some.inj (selectedAt.symm.trans pathAt)
    subst actualPath
    have transferReady : sourceRowReady data path sample.snapshot transfer = true :=
      Option.some.inj ((rows transfer transferMember).2.1.symm.trans actual)
    have same := checked_ready_owned_send_and_acceptance_name_the_same_action checked decoded
      selected transferMember member loanAt located transferred payload transferSending sending
      transferReady ready
    exact .inl ⟨rfl, sample, transfer, admitted, pathAt, transferMember, same.1, actual⟩
  · obtain ⟨sample, accepted, viewAt, agrees, pending⟩ := evidence
    have needs : sourceActionRequiresView sample.view intended target.actionId.side = true := by
      simpa only [viewAt] using required
    exact .inr ⟨.inl rfl, sample, accepted, viewAt,
      pending_source_use_prevents_live_view_retirement accepted decoded selected agrees
        member located needs pending⟩
  · obtain ⟨sample, accepted, viewAt, agrees, pending⟩ := evidence
    have needs : sourceActionRequiresView sample.view intended target.actionId.side = true := by
      simpa only [viewAt] using required
    exact .inr ⟨.inr rfl, sample, accepted, viewAt,
      pending_source_use_prevents_live_view_retirement accepted decoded selected agrees
        member located needs pending⟩

/-- Acceptance moves the sole held authority to exactly one frame. Completed
sender witnesses do not become a second authority. -/
theorem held_value_acceptance_publishes_one_frame
    {View Frame : Type} {value : View}
    {before after : EndpointTransfer.Observation View Frame}
    (step : EndpointTransfer.Step .accept before after)
    (held : TransferValueLocated value before) :
    ∃ frame, TransferFrameLocated frame after := by
  have bound : before.authorityCount ≤ 1 := by
    rcases held with ⟨usable, preparing, flight⟩ | ⟨usable, preparing, flight⟩
    all_goals simp only [EndpointTransfer.Observation.authorityCount, usable, preparing, flight,
      List.length_nil, List.length_cons]; decide
  have afterBound := Nat.le_trans (EndpointTransfer.transfer_step_cannot_duplicate_authority step) bound
  cases step with
  | accept state left right moved frame =>
      exact ⟨frame, one_queued_authority_is_the_exact_frame afterBound
        (List.mem_append_right _ (List.mem_cons_self))⟩

/-- Every finite suffix either keeps the exact value in application code or
a private future, or crosses a checked boundary for this source handoff/fault.
This classification does not assert eventual polling, admission or delivery. -/
theorem ready_source_handoff_keeps_its_value_or_takes_its_required_boundary
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {target : DecodedActorRow} {intended : Binding}
    {View Frame : Type} {value : View} {first final : EndpointTransfer.Observation View Frame}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, intended))
    (payload : intended.message.payload = .endpoint key)
    (sending : target.actionId.side = .send)
    (required : sourceActionRequiresView key.role intended target.actionId.side = true)
    (history : EndpointTransfer.Execution first final) (present : TransferValueLocated value first)
    (observed : ∀ prior next action,
      EndpointTransfer.Execution first prior → TransferValueLocated value prior →
      EndpointTransfer.Step action prior next →
      EndpointTransfer.Execution next final →
      SourceHandoffBoundaryEvidence image choreo data path key target action) :
    TransferValueLocated value final ∨
      ∃ prior next action, EndpointTransfer.Execution first prior ∧
        EndpointTransfer.Step action prior next ∧ TransferValueLocated value prior ∧
        EndpointTransfer.Execution next final ∧
        SourceHandoffBoundaryOutcome image choreo data path key target action ∧
        (action = .accept → ∃ frame, TransferFrameLocated frame next) := by
  rcases transfer_execution_keeps_the_value_until_handoff_or_retirement history present with
    kept | ⟨prior, next, action, earlier, boundary, held, kind, suffix⟩
  · exact .inl kept
  · refine .inr ⟨prior, next, action, earlier, boundary, held, suffix,
      checked_source_handoff_boundary_is_its_own_acceptance_or_a_fault checked decoded
        selected member located payload sending required (observed prior next action earlier held boundary suffix) kind, ?_⟩
    intro accepted
    subst action
    exact held_value_acceptance_publishes_one_frame boundary held

open EndpointTransfer

/-- Bind the suffix classification to an actual source-checked receipt and
ordered carrier. The preserved value is the exact issued receiver/view tag;
the next accepted handoff must name the intended source action, or a checked
retirement reports a fault. Complete Rust correspondence and liveness are
separate obligations, not consequences of this observation composition. -/
theorem source_checked_receipt_keeps_its_value_until_ready_handoff_or_fault
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (factsAt : sample.ownedFacts? image choreo = some facts) (receiving : facts.side = .receive)
    {data : DecodedActorOrder} {path : NormalizedPath} {target : DecodedActorRow} {intended : Binding}
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, intended))
    (payload : intended.message.payload = .endpoint facts.resource)
    (sending : target.actionId.side = .send)
    (required : sourceActionRequiresView facts.resource.role intended target.actionId.side = true)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered committed : carrier.State} {frame : TransportFrame} {before : TransportState}
    {state : Observation (Nat × Nat × Nat × Nat) TransportFrame} {rest : List TransportFrame}
    (initialView : Nat × Nat × Nat × Nat)
    (execution : Execution (initial initialView) state)
    (currentExact : abstraction current = .idle before) (wellFormed : before.WellFormed)
    (received : carrier.pollReceive current = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (identity : wire.identifies frame) (queued : state.inFlight = frame :: rest) :
    sample.after.actor = facts.binding.receiverActor ∧ sample.SourceOrdered image choreo ∧
      ∃ next after, Step .receive state next ∧
        TransferValueLocated (sample.after.actor, facts.afterView.header) next ∧
        abstraction committed = .idle after ∧ before.delivered < after.delivered ∧
        ∀ final, Execution next final →
          (∀ prior following action, Execution next prior →
            TransferValueLocated (sample.after.actor, facts.afterView.header) prior →
            Step action prior following →
            Execution following final →
            SourceHandoffBoundaryEvidence image choreo data path facts.resource target action) →
          TransferValueLocated (sample.after.actor, facts.afterView.header) final ∨
          ∃ prior following action, Execution next prior ∧ Step action prior following ∧
            TransferValueLocated (sample.after.actor, facts.afterView.header) prior ∧
            Execution following final ∧
            SourceHandoffBoundaryOutcome image choreo data path facts.resource target action ∧
            (action = .accept → ∃ acceptedFrame, TransferFrameLocated acceptedFrame following) := by
  obtain ⟨actor, ordered, next, after, receipt, valueAt, carrierAt, advances, _⟩ :=
    source_checked_receipt_keeps_the_exact_value_until_its_next_boundary checked factsAt receiving
      profile initialView execution currentExact wellFormed received resolved identity queued
  refine ⟨actor, ordered, next, after, receipt, valueAt, carrierAt, advances, ?_⟩
  intro final history observations
  exact ready_source_handoff_keeps_its_value_or_takes_its_required_boundary ordered.1 decoded
    (List.mem_of_getElem? pathAt) member located payload sending required history valueAt observations

end Hibana
