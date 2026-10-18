import Hibana.ActorAdmission
import Hibana.SourceResourceAvailability

namespace Hibana
open EndpointDelegation

theorem decoded_actor_order_fields
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    (decoded : image.decodeActorOrder? choreo = some data) :
    choreo.ownershipSource? = some data.source ∧
      normalize data.source = some data.normalized ∧
      image.decodeProgramColumns? = some data.columns ∧
      image.decodeScopeMarkers? = some data.markers ∧
      image.decodeActorRows? = some data.rows ∧ data.actor = image.role := by
  cases recoveryAt : image.ownedRecoveryIndexCheck with
  | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt] at decoded
  | true =>
      cases entryAt : image.ownedEntryResourcesCheck with
      | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt] at decoded
      | true =>
          simp only [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt,
            ite_true, Option.pure_def, Option.bind_eq_bind, Option.bind_some] at decoded
          simp only [Option.bind_eq_some_iff, Option.some.injEq] at decoded
          rcases decoded with ⟨source, sourceAt, normalized, normalizedAt,
            columns, columnsAt, markers, markersAt, rows, rowsAt,
            lanes, _, eventDependencies, _, dependencies, _, rfl⟩
          exact ⟨sourceAt, normalizedAt, columnsAt, markersAt, rowsAt, rfl⟩

private theorem member_actor_action_is_decoded
    {source : Source} {path : NormalizedPath} {actor : Nat}
    {id : ActionId} {action : Action}
    (member : (id, action) ∈ source.actorActions path actor) :
    action? path.bindings id = some action ∧ action.actor = actor := by
  obtain ⟨encoded, _, found⟩ := List.mem_filterMap.mp member
  cases atAction : action? path.bindings (decodeActionId encoded) with
  | none => simp [atAction] at found
  | some actual =>
      by_cases belongs : actual.actor = actor
      · have same : (decodeActionId encoded, actual) = (id, action) := by
          simpa [atAction, belongs] using found
        cases same
        exact ⟨atAction, belongs⟩
      · simp [atAction, belongs] at found

/-- The active decoded rows cover exactly the selected source actions at this
actor, in both directions. This is immutable translation validation. -/
theorem checked_active_rows_are_exactly_the_source_actions
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) :
    ((data.activeRows path).map (·.sourceAction data.actor)) =
      data.source.actorActions path data.actor := by
  obtain ⟨sourceAt, normalizedAt, columnsAt, markersAt, rowsAt, actorAt⟩ :=
    decoded_actor_order_fields decoded
  obtain ⟨source, normalized, columns, atoms, markers, rows,
    sourceFound, normalizedFound, columnsFound, _, markersFound, rowsFound, _, exactRows⟩ :=
    accepted_actor_rows_bind_source_operations checked
  have sourceSame := Option.some.inj (sourceFound.symm.trans sourceAt)
  subst source
  have normalizedSame := Option.some.inj (normalizedFound.symm.trans normalizedAt)
  subst normalized
  have columnsSame := Option.some.inj (columnsFound.symm.trans columnsAt)
  subst columns
  have markersSame := Option.some.inj (markersFound.symm.trans markersAt)
  subst markers
  have rowsSame := Option.some.inj (rowsFound.symm.trans rowsAt)
  subst rows
  have identities := exactRows path selected
  simpa only [DecodedActorOrder.activeRows, actorAt] using identities

/-- Every active decoded row has the source action at this same actor.
The actor equality is derived from exact translation validation, rather than
postulated as a resource holder table. -/
theorem active_decoded_row_has_its_exact_source_action
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {row : DecodedActorRow}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : row ∈ data.activeRows path) :
    action? path.bindings row.actionId = some (row.sourceAction data.actor).2 := by
  have identities := checked_active_rows_are_exactly_the_source_actions checked decoded selected
  have present : row.sourceAction data.actor ∈ data.source.actorActions path data.actor := by
    rw [← identities]
    exact List.mem_map.mpr ⟨row, member, rfl⟩
  exact (member_actor_action_is_decoded present).1

theorem instantiated_path_binding_has_source_input
    {run : Elaboration} {issuance : List Issuance} {bindings : List (Nat × Binding)}
    {event : Nat} {binding : Binding}
    (instantiated : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some bindings)
    (present : bindings.find? (fun entry => entry.1 == event) = some (event, binding)) :
    ∃ symbolic, (event, symbolic) ∈ run.bindings ∧
      instantiateBinding issuance symbolic = some binding := by
  obtain ⟨⟨occurrence, symbolic⟩, member, mapped⟩ := successful_option_mapM_has_input
    instantiated (event, binding) (List.mem_of_find?_eq_some present)
  cases bindingAt : instantiateBinding issuance symbolic with
  | none => simp [bindingAt] at mapped
  | some actual =>
      have same : (occurrence, actual) = (event, binding) := by simpa [bindingAt] using mapped
      cases same
      exact ⟨symbolic, member, bindingAt⟩

theorem source_order_gives_actor_precedence
    {data : DecodedActorOrder} {path : NormalizedPath} {before after : DecodedActorRow}
    (left : action? path.bindings before.actionId = some (before.sourceAction data.actor).2)
    (right : action? path.bindings after.actionId = some (after.sourceAction data.actor).2)
    (ordered : data.source.before before.actionId.event after.actionId.event = true) :
    precedes data.source path.bindings before.actionId after.actionId = true := by
  have different : before.actionId.event ≠ after.actionId.event :=
    Nat.ne_of_lt (source_order_strictly_increases_event ordered)
  have sameActor : (data.actor == data.actor) = true := decide_eq_true rfl
  have differentEvents : (before.actionId.event == after.actionId.event) = false :=
    decide_eq_false different
  simpa only [precedes, left, right, DecodedActorRow.sourceAction, sameActor,
    differentEvents, Bool.true_and, Bool.false_eq_true, ite_false] using ordered

theorem actor_precedence_is_a_source_predecessor
    {data : DecodedActorOrder} {path : NormalizedPath} {before after : DecodedActorRow}
    (left : before ∈ data.activeRows path) (right : after ∈ data.activeRows path)
    (ordered : precedes data.source path.bindings before.actionId after.actionId = true) :
    before.eventId ∈ data.sourcePredecessors path after.eventId := by
  apply List.mem_filterMap.mpr
  refine ⟨before, left, ?_⟩
  have hasAfter : (data.activeRows path).any (fun row => row.eventId == after.eventId &&
      precedes data.source path.bindings before.actionId row.actionId) = true :=
    List.any_eq_true.mpr ⟨after, right,
      Bool.and_eq_true_iff.mpr ⟨decide_eq_true rfl, ordered⟩⟩
  exact if_pos hasAfter

/-- For every accepted source and selected path, a distinct transfer of the
same resource conflicts with its active use. Parallel work on other resources
is unaffected. No finite execution bound or index-order schedule is assumed. -/
theorem accepted_resource_transfer_and_use_have_local_causal_order
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer useRow : DecodedActorRow}
    {loan useBinding : Binding} {key : EndpointKey}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path)
    (useMember : useRow ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (useAt : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (transferred : loan.message.payload = .endpoint key)
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (different : transfer.actionId.event ≠ useRow.actionId.event) :
    transfer.eventId ∈ data.sourcePredecessors path useRow.eventId ∨
      useRow.eventId ∈ data.sourcePredecessors path transfer.eventId := by
  obtain ⟨issuance, run, elaborated, _, instantiated⟩ :=
    decoded_actor_path_has_a_solved_source_derivation decoded selected
  obtain ⟨sent, sentAt, sentBinding⟩ :=
    instantiated_path_binding_has_source_input instantiated loanAt
  obtain ⟨required, requiredAt, requiredBinding⟩ :=
    instantiated_path_binding_has_source_input instantiated useAt
  have sentMessage := (instantiated_binding_retains_message_and_actor_references sentBinding).1
  have requiredMessage :=
    (instantiated_binding_retains_message_and_actor_references requiredBinding).1
  have order := accepted_source_orders_transfer_against_uses elaborated sentAt requiredAt
    (sentMessage ▸ transferred) (requiredMessage ▸ used)
  have transferAction := active_decoded_row_has_its_exact_source_action checked decoded selected transferMember
  have useAction := active_decoded_row_has_its_exact_source_action checked decoded selected useMember
  rcases order with same | before | after
  · exact False.elim (different same)
  · exact .inl (actor_precedence_is_a_source_predecessor transferMember useMember
      (source_order_gives_actor_precedence transferAction useAction (by simpa using before)))
  · exact .inr (actor_precedence_is_a_source_predecessor useMember transferMember
      (source_order_gives_actor_precedence useAction transferAction (by simpa using after)))

/-- A later local step cannot be ready before one of its source predecessors
completes. The predecessor need not itself already be ready. -/
theorem a_pending_source_predecessor_blocks_its_successor
    {data : DecodedActorOrder} {path : NormalizedPath} {before after : DecodedActorRow}
    {snapshot : ActorProgressObservation}
    (parent : before.eventId ∈ data.sourcePredecessors path after.eventId)
    (pending : snapshot.doneState.done before.eventId = false) :
    sourceRowReady data path snapshot after = false := by
  cases ready : sourceRowReady data path snapshot after with
  | false => rfl
  | true =>
      have done := List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 before.eventId parent
      rw [pending] at done
      contradiction

theorem source_ready_use_excludes_a_distinct_resource_transfer
    {data : DecodedActorOrder} {path : NormalizedPath} {transfer useRow : DecodedActorRow}
    {snapshot : ActorProgressObservation}
    (ordered : transfer.eventId ∈ data.sourcePredecessors path useRow.eventId ∨
      useRow.eventId ∈ data.sourcePredecessors path transfer.eventId)
    (ready : sourceRowReady data path snapshot useRow = true) :
    sourceRowReady data path snapshot transfer = false := by
  have useReady := Bool.and_eq_true_iff.mp ready
  have pending : snapshot.doneState.done useRow.eventId = false := by simpa using useReady.1
  rcases ordered with before | after
  · have completed := List.all_eq_true.mp useReady.2 transfer.eventId before
    simp [sourceRowReady, completed]
  · exact a_pending_source_predecessor_blocks_its_successor after pending

theorem checked_source_ready_use_excludes_another_handoff
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {transfer useRow : DecodedActorRow}
    {loan useBinding : Binding} {key : EndpointKey} {snapshot : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (transferMember : transfer ∈ data.activeRows path)
    (useMember : useRow ∈ data.activeRows path)
    (loanAt : path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
      some (transfer.actionId.event, loan))
    (useAt : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (transferred : loan.message.payload = .endpoint key)
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (notHandoff : useBinding.message.payload ≠ .endpoint key)
    (ready : sourceRowReady data path snapshot useRow = true) :
    sourceRowReady data path snapshot transfer = false := by
  have different : transfer.actionId.event ≠ useRow.actionId.event := by
    intro same
    have found := loanAt
    rw [same] at found
    have bindingsSame := congrArg Prod.snd (Option.some.inj (found.symm.trans useAt))
    change loan = useBinding at bindingsSame
    apply notHandoff
    simpa only [← bindingsSame] using transferred
  exact source_ready_use_excludes_a_distinct_resource_transfer
    (accepted_resource_transfer_and_use_have_local_causal_order checked decoded selected
      transferMember useMember loanAt useAt transferred used different) ready

/-- A checked retirement of a still-needed view cannot leave this run live.
The pending requirement comes from the same decoded source and selected path. -/
theorem pending_source_use_prevents_live_view_retirement
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {binding : Binding}
    (checked : sample.check image choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (agrees : observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true)
    (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (required : sourceActionRequiresView sample.view binding row.actionId.side = true)
    (pending : sample.before.doneState.done row.eventId = false) :
    sample.afterFault ≠ none := by
  intro live
  obtain ⟨actual, actualAt, complete⟩ := live_retirement_removes_no_pending_source_action checked live
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  obtain ⟨entry, step, entryAt, stepAt, completed⟩ := complete path selected agrees row member
  have entrySame := Option.some.inj (entryAt.symm.trans located)
  subst entry
  have done := completed required
  simp [ActorProgressObservation.doneState, ActorProgressObservation.progress,
    List.getElem?_map, stepAt, done] at pending

/-- Checked observations at a live, source-ready resource-use cut. Acceptance
is tied to an actual cursor result before transport acceptance, so this does
not require the sender's later publication before a remote receipt. Retirement
supplies the actual first-fault observation. These are proof inputs, not a
runtime owner table, flag, or a rule for scheduling application code. -/
def SourceUseBoundaryEvidence
    (image : RustDescriptorImage) (choreo : Choreo) (data : DecodedActorOrder)
    (path : NormalizedPath) (key : EndpointKey) (useRow : DecodedActorRow)
    (action : EndpointTransfer.Action) : Prop :=
  match action with
  | .accept =>
      ∃ sample : ActorAdmissionObservation, ∃ transfer : DecodedActorRow, ∃ loan : Binding,
        sample.check image choreo = true ∧
        data.normalized.paths[sample.pathIndex]? = some path ∧
        transfer ∈ data.activeRows path ∧
        transfer.actionId.side = .send ∧
        path.bindings.find? (fun entry => entry.1 == transfer.actionId.event) =
          some (transfer.actionId.event, loan) ∧
        loan.message.payload = .endpoint key ∧
        sample.admitted[transfer.eventId]? = some true ∧
        sourceRowReady data path sample.snapshot useRow = true
  | .abandonPreparation | .dropValue =>
      ∃ sample : ViewRetirementObservation,
        sample.check image choreo = true ∧ sample.view = key.role ∧
        observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true ∧
        sample.before.doneState.done useRow.eventId = false ∧ sample.afterFault = none
  | .seal | .receive | .retireAcceptedFuture | .fenceFrame | .unchanged => True

theorem checked_live_source_cut_cannot_exit_by_handoff_or_retirement
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {useRow : DecodedActorRow}
    {useBinding : Binding} {action : EndpointTransfer.Action}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : useRow ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (required : sourceActionRequiresView key.role useBinding useRow.actionId.side = true)
    (notHandoff : useBinding.message.payload ≠ .endpoint key)
    (evidence : SourceUseBoundaryEvidence image choreo data path key useRow action) :
    action ≠ .accept ∧ action ≠ .abandonPreparation ∧ action ≠ .dropValue := by
  have noAccept : action ≠ .accept := by
    intro accepted
    subst action
    obtain ⟨sample, transfer, loan, admitted, pathAt, transferMember, _sending, loanAt,
      transferred, actual, ready⟩ := evidence
    obtain ⟨sameData, samePath, dataAt, chosenAt, exactRows⟩ :=
      accepted_actor_admission_has_exact_source_readiness admitted
    have dataSame := Option.some.inj (dataAt.symm.trans decoded)
    subst sameData
    have pathSame := Option.some.inj (chosenAt.symm.trans pathAt)
    subst samePath
    have transferReady : sourceRowReady data path sample.snapshot transfer = true :=
      Option.some.inj ((exactRows transfer transferMember).2.1.symm.trans actual)
    have blocked := checked_source_ready_use_excludes_another_handoff checked decoded selected
      transferMember member loanAt located transferred used notHandoff ready
    rw [blocked] at transferReady
    contradiction
  have noRetirement : ∀ retired,
      retired = EndpointTransfer.Action.abandonPreparation ∨ retired = .dropValue → action ≠ retired := by
    intro retired kind same
    subst action
    rcases kind with same | same <;> subst retired
    all_goals
      obtain ⟨sample, accepted, viewAt, agrees, pending, live⟩ := evidence
      have requirement : sourceActionRequiresView sample.view useBinding useRow.actionId.side = true := by
        simpa only [viewAt] using required
      exact pending_source_use_prevents_live_view_retirement accepted decoded selected agrees
        member located requirement pending live
  exact ⟨noAccept, noRetirement _ (.inl rfl), noRetirement _ (.inr rfl)⟩

/-- For an arbitrary finite transfer suffix while a source use stays ready
and its run live, preserve the exact received value in application code or a
private send future. An accepted later handoff cannot silently take it away,
and a checked drop cannot stay live. Fresh visits, complete observation
extraction, and eventual future polling remain separate obligations. -/
theorem checked_live_source_suffix_preserves_the_exact_received_value
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {key : EndpointKey} {useRow : DecodedActorRow} {useBinding : Binding}
    {View Frame : Type} {value : View} {first final : EndpointTransfer.Observation View Frame}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : useRow ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (used : useBinding.message.sender = key ∨ useBinding.message.receiver = key ∨
      useBinding.message.payload = .endpoint key)
    (required : sourceActionRequiresView key.role useBinding useRow.actionId.side = true)
    (notHandoff : useBinding.message.payload ≠ .endpoint key)
    (history : EndpointTransfer.Execution first final) (present : TransferValueLocated value first)
    (observed : ∀ prior next action,
      EndpointTransfer.Execution first prior → EndpointTransfer.Step action prior next →
      EndpointTransfer.Execution next final →
      SourceUseBoundaryEvidence image choreo data path key useRow action) :
    TransferValueLocated value final := by
  rcases transfer_execution_keeps_the_value_until_handoff_or_retirement history present with
    kept | ⟨prior, next, action, earlier, boundary, _, kind, suffix⟩
  · exact kept
  · have excludes := checked_live_source_cut_cannot_exit_by_handoff_or_retirement checked decoded
      selected member located used required notHandoff (observed prior next action earlier boundary suffix)
    rcases kind with accept | abandon | dropped
    · exact False.elim (excludes.1 accept)
    · exact False.elim (excludes.2.1 abandon)
    · exact False.elim (excludes.2.2 dropped)

open EndpointTransfer

/-- Compose the ordered carrier receipt with source-cut preservation. The
retained value is the actual projected receiver/view from the checked receipt,
not an arbitrary replacement capability. This still requires checked boundary
observations for the suffix; it is not automatic extraction of every Rust run. -/
theorem source_checked_receipt_retains_value_at_a_live_source_use_cut
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (factsAt : sample.ownedFacts? image choreo = some facts)
    (receiving : facts.side = .receive)
    {data : DecodedActorOrder} {path : NormalizedPath} {useRow : DecodedActorRow}
    {useBinding : Binding}
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : useRow ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == useRow.actionId.event) =
      some (useRow.actionId.event, useBinding))
    (used : useBinding.message.sender = facts.resource ∨ useBinding.message.receiver = facts.resource ∨
      useBinding.message.payload = .endpoint facts.resource)
    (required : sourceActionRequiresView facts.resource.role useBinding useRow.actionId.side = true)
    (notHandoff : useBinding.message.payload ≠ .endpoint facts.resource)
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
          (∀ prior following action, Execution next prior → Step action prior following →
            Execution following final →
            SourceUseBoundaryEvidence image choreo data path facts.resource useRow action) →
          TransferValueLocated (sample.after.actor, facts.afterView.header) final := by
  obtain ⟨actor, ordered, next, after, receipt, valueAt, carrierAt, advances, _⟩ :=
    source_checked_receipt_keeps_the_exact_value_until_its_next_boundary checked factsAt receiving
      profile initialView execution currentExact wellFormed received resolved identity queued
  refine ⟨actor, ordered, next, after, receipt, valueAt, carrierAt, advances, ?_⟩
  intro final history observations
  exact checked_live_source_suffix_preserves_the_exact_received_value ordered.1 decoded
    (List.mem_of_getElem? pathAt) member located used required notHandoff history valueAt observations

end Hibana
