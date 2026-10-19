import Hibana.SourceHandoffInterval

namespace Hibana
open EndpointDelegation

/-- Source-derived prerequisite using a view held by a pending call. This is
erased proof evidence for the production classifier, not an owner table. -/
def sourceHeldViewPending (data : DecodedActorOrder) (path : NormalizedPath)
    (snapshot : ActorProgressObservation) (target : DecodedActorRow) (role : Nat) : Bool :=
  (data.activeRows path).any fun prior =>
    !snapshot.doneState.done prior.eventId &&
      decide (prior.eventId ∈ data.sourcePredecessors path target.eventId) &&
        match path.bindings.find? (fun entry => entry.1 == prior.actionId.event) with
        | none => false
        | some (_, binding) => sourceActionRequiresView role binding prior.actionId.side

theorem a_source_held_view_wait_has_an_unfinished_required_predecessor
    {data : DecodedActorOrder} {path : NormalizedPath} {snapshot : ActorProgressObservation}
    {target : DecodedActorRow} {role : Nat}
    (blocked : sourceHeldViewPending data path snapshot target role = true) :
    ∃ prior binding, prior ∈ data.activeRows path ∧
      snapshot.doneState.done prior.eventId = false ∧
      prior.eventId ∈ data.sourcePredecessors path target.eventId ∧
      path.bindings.find? (fun entry => entry.1 == prior.actionId.event) =
        some (prior.actionId.event, binding) ∧
      sourceActionRequiresView role binding prior.actionId.side = true := by
  obtain ⟨prior, member, fields⟩ := List.any_eq_true.mp blocked
  have outside := Bool.and_eq_true_iff.mp fields
  have first := Bool.and_eq_true_iff.mp outside.1
  have pending : snapshot.doneState.done prior.eventId = false := by
    simpa using first.1
  have parent := of_decide_eq_true first.2
  cases located : path.bindings.find? (fun entry => entry.1 == prior.actionId.event) with
  | none => simp [located] at fields
  | some entry =>
      have eventAt : (entry.1 == prior.actionId.event) = true :=
        List.find?_some (p := fun pair : Nat × Binding => pair.1 == prior.actionId.event) located
      refine ⟨prior, entry.2, member, pending, parent, ?_, ?_⟩
      · exact located.trans (congrArg some (Prod.ext (beq_iff_eq.mp eventAt) rfl))
      · simpa only [located] using outside.2

/-- If a required use is ready, every distinct unfinished loan is after it.
The event coordinates do not order independent work: the order is recovered
from accepted resource ownership and the source dependency relation. -/
theorem a_ready_resource_use_precedes_every_pending_conflicting_handoff
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer useRow : DecodedActorRow}
    {loan useBinding : Binding} {key : EndpointKey} {snapshot : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path) (useMember : useRow ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (useAt : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (payload : loan.message.payload = .endpoint key)
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (different : transfer.actionId.event ≠ useRow.actionId.event)
    (pending : snapshot.doneState.done transfer.eventId = false)
    (ready : sourceRowReady data path snapshot useRow = true) :
    useRow.eventId ∈ data.sourcePredecessors path transfer.eventId := by
  rcases accepted_resource_transfer_and_use_have_local_causal_order checked decoded selected
    transferMember useMember loanAt useAt payload used different with before | after
  · have done := List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 transfer.eventId before
    rw [pending] at done
    contradiction
  · exact after

theorem a_ready_resource_use_blocks_a_pending_handoff_with_its_held_payload
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer useRow : DecodedActorRow}
    {loan useBinding : Binding} {key : EndpointKey} {snapshot : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path) (useMember : useRow ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (useAt : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (payload : loan.message.payload = .endpoint key)
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (required : sourceActionRequiresView key.role useBinding useRow.actionId.side = true)
    (different : transfer.actionId.event ≠ useRow.actionId.event)
    (pending : snapshot.doneState.done transfer.eventId = false)
    (ready : sourceRowReady data path snapshot useRow = true) :
    sourceHeldViewPending data path snapshot transfer key.role = true := by
  have parent := a_ready_resource_use_precedes_every_pending_conflicting_handoff checked decoded
    selected transferMember useMember loanAt useAt payload used different pending ready
  apply List.any_eq_true.mpr
  refine ⟨useRow, useMember, ?_⟩
  have unfinished : (!snapshot.doneState.done useRow.eventId) = true :=
    (Bool.and_eq_true_iff.mp ready).1
  simp only [unfinished, Bool.true_and, decide_eq_true parent, useAt, required]

/-- Immutable source facts at an initial issuance or actual receipt boundary.
This proposition is erased proof evidence, not a runtime owner ledger. The
receipt's current-path prefix closure remains an explicit source obligation. -/
def SourceIntervalOrigin (data : DecodedActorOrder) (path : NormalizedPath)
    (target : DecodedActorRow) (binding : Binding) (key : EndpointKey)
    (first : ActorProgressObservation) : Prop :=
  ∃ (issuance : List Issuance) (run : Elaboration) (index : Nat) (symbolic : SymbolicBinding),
    elaborate data.source.resources (initialOwners data.source.resources)
      path.selections data.source 0 = some run ∧
    run.bindings[index]? = some (target.actionId.event, symbolic) ∧
    instantiateBinding issuance symbolic = some binding ∧
    (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some path.bindings ∧
    ((instantiate issuance ((initialOwners data.source.resources) key) = some data.actor ∧
        ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint key) ∨
      ∃ receipt previous earlierRows laterRows,
        receipt ∈ data.activeRows path ∧ receipt.actionId.side = .receive ∧
        run.bindings.take index = earlierRows ++ (receipt.actionId.event, previous) :: laterRows ∧
        previous.message.payload = .endpoint key ∧
        (∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) ∧
        first.doneState.done receipt.eventId = true ∧
        first.doneState.PrefixClosed (data.sourcePredecessors path) [receipt.eventId])

/-- Copied source observations at the actual boundaries of one binding's
transfer execution. Acceptance uses actual admission plus measured histories;
retirement uses its actual checked first fault. No target-readiness premise is
required at an intermediate cut. Authentic same-run correspondence is still
an implementation contract, not established by constructing this proposition. -/
def MeasuredResourceBoundaryEvidence
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (data : DecodedActorOrder) (path : NormalizedPath) (key : EndpointKey)
    (first : ActorProgressObservation) (firstCount : Nat)
    (final : ActorProgressObservation) (finalCount : Nat)
    (action : EndpointTransfer.Action) : Prop :=
  match action with
  | .accept =>
      ∃ (sample : ActorAdmissionObservation) (transfer : DecodedActorRow) (loan : Binding)
        (cutCount : Nat) (prior later : List ActorCommitMutationRecord),
        sample.check image choreo = true ∧
        data.normalized.paths[sample.pathIndex]? = some path ∧
        transfer ∈ data.activeRows path ∧ transfer.actionId.side = .send ∧
        path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
          some (transfer.actionId.event, loan) ∧
        loan.message.payload = .endpoint key ∧ sample.admitted[transfer.eventId]? = some true ∧
        actorCommitMutationHistoryCheck image choreo session first firstCount
          sample.snapshot cutCount prior = true ∧
        actorCommitMutationHistoryCheck image choreo session sample.snapshot cutCount
          final finalCount later = true
  | .abandonPreparation | .dropValue =>
      ∃ (sample : ViewRetirementObservation) (cutCount : Nat) (later : List ActorCommitMutationRecord),
        sample.check image choreo = true ∧ sample.view = key.role ∧
        observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true ∧
        actorCommitMutationHistoryCheck image choreo session sample.before cutCount
          final finalCount later = true
  | .seal | .receive | .retireAcceptedFuture | .fenceFrame | .unchanged => True

/-- Acceptance identifies the target global message, separately from its
sender's local publication. For a sending target this is its exact handoff;
an ordinary message cannot match a loan payload. Retirement records a fault. -/
def SourceMessageBoundaryOutcome
    (image : RustDescriptorImage) (choreo : Choreo) (data : DecodedActorOrder)
    (path : NormalizedPath) (key : EndpointKey) (target : DecodedActorRow)
    (action : EndpointTransfer.Action) : Prop :=
  (action = .accept ∧ ∃ (sample : ActorAdmissionObservation) (transfer : DecodedActorRow) (loan : Binding),
    sample.check image choreo = true ∧ data.normalized.paths[sample.pathIndex]? = some path ∧
    transfer ∈ data.activeRows path ∧ transfer.actionId.side = .send ∧
    path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan) ∧
    loan.message.payload = .endpoint key ∧ transfer.actionId.event = target.actionId.event ∧
    sample.admitted[transfer.eventId]? = some true) ∨
  ((action = .abandonPreparation ∨ action = .dropValue) ∧
    ∃ sample : ViewRetirementObservation,
      sample.check image choreo = true ∧ sample.view = key.role ∧ sample.after.fault ≠ none)

private theorem an_origin_interval_excludes_a_distinct_handoff
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {target transfer : DecodedActorRow} {binding loan : Binding} {key : EndpointKey}
    {first cut final : ActorProgressObservation} {firstCount cutCount finalCount : Nat}
    {prior later : List ActorCommitMutationRecord}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (origin : SourceIntervalOrigin data path target binding key first)
    (transferMember : transfer ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (payload : loan.message.payload = .endpoint key)
    (used : binding.message.sender = key ∨ binding.message.receiver = key ∨ binding.message.payload = .endpoint key)
    (sending : transfer.actionId.side = .send)
    (different : transfer.actionId.event ≠ target.actionId.event)
    (prefixChecked : actorCommitMutationHistoryCheck image choreo session
      first firstCount cut cutCount prior = true)
    (suffixChecked : actorCommitMutationHistoryCheck image choreo session
      cut cutCount final finalCount later = true)
    (pending : final.doneState.done target.eventId = false) :
    sourceRowReady data path cut transfer = false := by
  obtain ⟨issuance, run, index, symbolic, elaborated, sourceTarget, instantiated, mapped, boundary⟩ := origin
  rcases boundary with ⟨_, untouched⟩ |
    ⟨receipt, previous, earlierRows, laterRows, receiptMember, receiving, shape, previousPayload,
      untouched, receiptDone, receiptClosed⟩
  · exact a_measured_initial_resource_interval_excludes_a_distinct_handoff checked decoded selected
      member transferMember loanAt payload used different elaborated sourceTarget instantiated mapped
      untouched suffixChecked pending
  · exact a_measured_last_receipt_interval_excludes_a_distinct_handoff checked decoded selected
      member transferMember receiptMember loanAt payload used sending receiving different elaborated
      sourceTarget instantiated mapped shape previousPayload untouched receiptDone receiptClosed
      prefixChecked suffixChecked pending

theorem a_measured_resource_boundary_is_its_source_message_or_a_fault
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {target : DecodedActorRow} {binding : Binding} {key : EndpointKey}
    {first final : ActorProgressObservation} {firstCount finalCount : Nat}
    {action : EndpointTransfer.Action}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, binding))
    (used : binding.message.sender = key ∨ binding.message.receiver = key ∨ binding.message.payload = .endpoint key)
    (required : sourceActionRequiresView key.role binding target.actionId.side = true)
    (origin : SourceIntervalOrigin data path target binding key first)
    (pending : final.doneState.done target.eventId = false)
    (evidence : MeasuredResourceBoundaryEvidence image choreo session data path key
      first firstCount final finalCount action)
    (kind : action = .accept ∨ action = .abandonPreparation ∨ action = .dropValue) :
    SourceMessageBoundaryOutcome image choreo data path key target action := by
  rcases kind with rfl | rfl | rfl
  · obtain ⟨sample, transfer, loan, cutCount, prior, later, admitted, pathAt, transferMember,
      sending, loanAt, payload, actual, prefixChecked, suffixChecked⟩ := evidence
    obtain ⟨actualData, actualPath, actualAt, selectedAt, rows⟩ :=
      accepted_actor_admission_has_exact_source_readiness admitted
    have dataSame := Option.some.inj (actualAt.symm.trans decoded)
    subst actualData
    have pathSame := Option.some.inj (selectedAt.symm.trans pathAt)
    subst actualPath
    have ready : sourceRowReady data path sample.snapshot transfer = true :=
      Option.some.inj ((rows transfer transferMember).2.1.symm.trans actual)
    have same : transfer.actionId.event = target.actionId.event := by
      by_cases same : transfer.actionId.event = target.actionId.event
      · exact same
      · have blocked := an_origin_interval_excludes_a_distinct_handoff checked decoded selected
          member origin transferMember loanAt payload used sending same prefixChecked suffixChecked pending
        rw [blocked] at ready
        contradiction
    exact .inl ⟨rfl, sample, transfer, loan, admitted, pathAt, transferMember, sending,
      loanAt, payload, same, actual⟩
  · obtain ⟨sample, cutCount, later, accepted, viewAt, agrees, suffixChecked⟩ := evidence
    have needs : sourceActionRequiresView sample.view binding target.actionId.side = true := by
      simpa only [viewAt] using required
    exact .inr ⟨.inl rfl, sample, accepted, viewAt,
      pending_source_use_prevents_live_view_retirement accepted decoded selected agrees member located needs
        (a_pending_final_event_was_pending_at_every_measured_commit suffixChecked pending).1⟩
  · obtain ⟨sample, cutCount, later, accepted, viewAt, agrees, suffixChecked⟩ := evidence
    have needs : sourceActionRequiresView sample.view binding target.actionId.side = true := by
      simpa only [viewAt] using required
    exact .inr ⟨.inr rfl, sample, accepted, viewAt,
      pending_source_use_prevents_live_view_retirement accepted decoded selected agrees member located needs
        (a_pending_final_event_was_pending_at_every_measured_commit suffixChecked pending).1⟩

/-- For any finite matching transfer execution, keep the exact first value in
app code/private custody, accept the required source message, or report an
actual checked retirement fault. There is no intermediate target-ready oracle.
Correspondence of source cuts to these same transfer boundaries remains required. -/
theorem a_measured_source_interval_keeps_its_value_or_takes_its_boundary
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {target : DecodedActorRow} {binding : Binding} {key : EndpointKey}
    {firstProgress finalProgress : ActorProgressObservation} {firstCount finalCount : Nat}
    {View Frame : Type} {value : View} {first final : EndpointTransfer.Observation View Frame}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, binding))
    (used : binding.message.sender = key ∨ binding.message.receiver = key ∨ binding.message.payload = .endpoint key)
    (required : sourceActionRequiresView key.role binding target.actionId.side = true)
    (origin : SourceIntervalOrigin data path target binding key firstProgress)
    (pending : finalProgress.doneState.done target.eventId = false)
    (history : EndpointTransfer.Execution first final) (present : TransferValueLocated value first)
    (observed : ∀ prior next action,
      EndpointTransfer.Execution first prior → TransferValueLocated value prior →
      EndpointTransfer.Step action prior next → EndpointTransfer.Execution next final →
      MeasuredResourceBoundaryEvidence image choreo session data path key
        firstProgress firstCount finalProgress finalCount action) :
    TransferValueLocated value final ∨
      ∃ prior next action, EndpointTransfer.Execution first prior ∧
        EndpointTransfer.Step action prior next ∧ TransferValueLocated value prior ∧
        EndpointTransfer.Execution next final ∧
        SourceMessageBoundaryOutcome image choreo data path key target action ∧
        (action = .accept → ∃ frame, TransferFrameLocated frame next) := by
  rcases transfer_execution_keeps_the_value_until_handoff_or_retirement history present with
    kept | ⟨prior, next, action, earlier, boundary, held, kind, suffix⟩
  · exact .inl kept
  · refine .inr ⟨prior, next, action, earlier, boundary, held, suffix,
      a_measured_resource_boundary_is_its_source_message_or_a_fault checked decoded selected
        member located used required origin pending (observed prior next action earlier held boundary suffix) kind, ?_⟩
    intro accepted
    subst action
    exact held_value_acceptance_publishes_one_frame boundary held

/-- For an ordinary target, the acceptance alternative is impossible. Any
retirement instead contains its real checked fault. -/
theorem the_measured_boundary_of_an_ordinary_use_is_a_fault
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {target : DecodedActorRow} {binding : Binding}
    {action : EndpointTransfer.Action}
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, binding))
    (ordinary : binding.message.payload ≠ .endpoint key)
    (outcome : SourceMessageBoundaryOutcome image choreo data path key target action) :
    (action = .abandonPreparation ∨ action = .dropValue) ∧
      ∃ sample : ViewRetirementObservation,
        sample.check image choreo = true ∧ sample.view = key.role ∧ sample.after.fault ≠ none := by
  rcases outcome with ⟨_, sample, transfer, loan, _, _, _, _, loanAt, payload, same, _⟩ | retired
  · rw [same] at loanAt
    have sameBinding := congrArg Prod.snd (Option.some.inj (loanAt.symm.trans located))
    change loan = binding at sameBinding
    exact False.elim (ordinary (by simpa only [← sameBinding] using payload))
  · exact retired

theorem the_measured_boundary_of_a_sending_target_is_its_exact_handoff_or_fault
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {target : DecodedActorRow}
    {action : EndpointTransfer.Action}
    (sending : target.actionId.side = .send)
    (outcome : SourceMessageBoundaryOutcome image choreo data path key target action) :
    SourceHandoffBoundaryOutcome image choreo data path key target action := by
  rcases outcome with ⟨accepted, sample, transfer, loan, checked, pathAt, member, transferSending,
    _, _, same, actual⟩ | retired
  · have first := congrArg (fun event => ActionId.mk event transfer.actionId.side) same
    have second := congrArg (ActionId.mk target.actionId.event) (transferSending.trans sending.symm)
    exact .inl ⟨accepted, sample, transfer, checked, pathAt, member, first.trans second, actual⟩
  · exact .inr retired

open EndpointTransfer

/-- Derive the first value from the actual source-checked ordered receipt,
then classify its complete measured transfer suffix. There is no value-
availability premise at the final ready cut, nor readiness at earlier cuts.

The prefix/visit and implementation-to-boundary correspondences remain
required. This establishes custody or a concrete progress/fault boundary;
eventual polling of private futures and cross-visit progress are separate. -/
theorem a_checked_receipt_retains_its_value_until_a_measured_source_boundary
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (factsAt : sample.ownedFacts? image choreo = some facts) (receiving : facts.side = .receive)
    {data : DecodedActorOrder} {path : NormalizedPath} {target : DecodedActorRow} {binding : Binding}
    {firstCount finalCount : Nat} {finalProgress : ActorProgressObservation}
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : target ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == target.actionId.event) =
      some (target.actionId.event, binding))
    (used : binding.message.sender = facts.resource ∨ binding.message.receiver = facts.resource ∨
      binding.message.payload = .endpoint facts.resource)
    (required : sourceActionRequiresView facts.resource.role binding target.actionId.side = true)
    (origin : SourceIntervalOrigin data path target binding facts.resource sample.after)
    (ready : sourceRowReady data path finalProgress target = true)
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
            Step action prior following → Execution following final →
            MeasuredResourceBoundaryEvidence image choreo session data path facts.resource
              sample.after firstCount finalProgress finalCount action) →
          TransferValueLocated (sample.after.actor, facts.afterView.header) final ∨
          ∃ prior following action, Execution next prior ∧ Step action prior following ∧
            TransferValueLocated (sample.after.actor, facts.afterView.header) prior ∧
            Execution following final ∧
            SourceMessageBoundaryOutcome image choreo data path facts.resource target action ∧
            (action = .accept → ∃ acceptedFrame, TransferFrameLocated acceptedFrame following) := by
  obtain ⟨actor, ordered, next, after, receipt, present, carrierAt, advances, _⟩ :=
    source_checked_receipt_keeps_the_exact_value_until_its_next_boundary checked factsAt receiving
      profile initialView execution currentExact wellFormed received resolved identity queued
  have pending : finalProgress.doneState.done target.eventId = false := by
    cases doneAt : finalProgress.doneState.done target.eventId with
    | false => rfl
    | true =>
        have negated := (Bool.and_eq_true_iff.mp ready).1
        rw [doneAt] at negated
        contradiction
  refine ⟨actor, ordered, next, after, receipt, present, carrierAt, advances, ?_⟩
  intro final history observations
  exact a_measured_source_interval_keeps_its_value_or_takes_its_boundary ordered.1 decoded selected
    member located used required origin pending history present observations

end Hibana
