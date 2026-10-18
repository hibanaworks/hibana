import Hibana.ActorCommit
import Hibana.ActorReentry
import Hibana.SourceResourceAvailability

namespace Hibana
open EndpointDelegation

/-- An atomic body reentry and its successful operation. These are copied
observations; the prepared base is derived, never stored in the Rust runtime. -/
structure ActorReentryCommitObservation where
  previousPath : Nat
  nextPath : Nat
  scope : Nat
  event : Nat
  view : Nat
  before : ActorProgressObservation
  after : ActorProgressObservation

def observedEntryHeaderMatches
    (snapshot : ActorProgressObservation) (role owner : Nat) : Bool :=
  if owner = snapshot.actor then snapshot.viewIssued role else
    match observedActorView? snapshot role with
    | none => true
    | some view => view.run == 0 && view.nonce % 2 == 0

def observedEntryHeadersMatch
    (snapshot : ActorProgressObservation) (columns : DecodedProgramColumns)
    (entries : List (Nat × Nat)) : Bool :=
  entries.all fun (role, reference) =>
    match columns.selectedActor? snapshot.doneState.selected reference with
    | none => false
    | some owner => observedEntryHeaderMatches snapshot role owner

/-- Entry references are checked against the complete source-derived table by
decodeActorOrder?. Inspect the existing headers before clearing this visit. -/
def observedRollEntryHeadersMatch
    (snapshot : ActorProgressObservation) (image : RustDescriptorImage) (scope : Nat) : Bool :=
  match image.decodeProgramColumns?, image.decodeRollEntryResources? with
  | some columns, some rows =>
    match resolveUnique? (rows.filter fun row => row.1.scope == scope) with
    | none => false
    | some (_, entries) => observedEntryHeadersMatch snapshot columns entries
  | _, _ => false

/-- The prospective owned payload comes from the immutable source atom, not
from a runtime decision or the contents of a received frame. -/
def observedOutgoingPayload?
    (image : RustDescriptorImage) (event view : Nat) : Option (Option Nat) := do
  let eff ← image.eventEffIndex? event
  let atoms ← image.decodeAtoms?
  let atom ← resolveUnique? (atoms.filter fun atom => atom.effIndex == eff)
  if atom.sender != view || atom.schema < 4294967296 then pure none
  else if atom.schema < 4294967552 then pure (some (atom.schema - 4294967296))
  else none

def observedEntryBorrowMatches
    (snapshot : ActorProgressObservation) (entries : List (Nat × Nat))
    (sent : Option Nat) : Bool :=
  entries.all fun (role, _) =>
    match observedActorView? snapshot role with
    | none => true
    | some view => checkSendPayloadBorrow view.operation role sent

def observedRollEntryBorrowMatches
    (snapshot : ActorProgressObservation) (image : RustDescriptorImage)
    (scope event view : Nat) : Bool :=
  match image.decodeRollEntryResources?, observedOutgoingPayload? image event view with
  | some rows, some sent =>
    match resolveUnique? (rows.filter fun row => row.1.scope == scope) with
    | none => false
    | some (_, entries) => observedEntryBorrowMatches snapshot entries sent
  | _, _ => false

theorem observed_entry_borrow_requires_its_own_send
    {snapshot : ActorProgressObservation} {entries : List (Nat × Nat)}
    {role reference : Nat} {view : ActorViewObservation} {sent : Option Nat}
    (checked : observedEntryBorrowMatches snapshot entries sent = true)
    (member : (role, reference) ∈ entries)
    (found : observedActorView? snapshot role = some view)
    (borrowed : view.operation = .sendPayload) : sent = some role := by
  have one := List.all_eq_true.mp checked (role, reference) member
  apply a_pending_payload_requires_its_own_send
  simpa only [found, borrowed] using one

theorem observed_entry_header_requires_issued_resource
    {snapshot : ActorProgressObservation} {columns : DecodedProgramColumns}
    {entries : List (Nat × Nat)} {role reference : Nat}
    (checked : observedEntryHeadersMatch snapshot columns entries = true)
    (member : (role, reference) ∈ entries)
    (resolved : columns.selectedActor? snapshot.doneState.selected reference = some snapshot.actor) :
    ∃ view, observedActorView? snapshot role = some view ∧
      view.run = snapshot.run ∧ view.nonce % 2 = 1 := by
  have one := List.all_eq_true.mp checked (role, reference) member
  apply observed_issued_view_has_current_header
  simpa only [resolved, observedEntryHeaderMatches, if_pos rfl] using one

def resetObservedActor
    (snapshot : ActorProgressObservation) (roll : DecodedRollRow)
    (choices : List (Nat × Option RouteArm)) : ActorProgressObservation :=
  { snapshot with
    choices
    steps := snapshot.steps.zipIdx.map fun (step, event) =>
      { step with done := if roll.eventStart ≤ event &&
          event < roll.eventStart + roll.eventLength then false else step.done } }

def resetObservedChoices?
    (source : CanonicalProgramSource) (scope : Nat)
    (choices : List (Nat × Option RouteArm)) : Option (List (Nat × Option RouteArm)) :=
  choices.mapM fun (route, arm) => do
    let reset ← sourceRollResetsRoute? source scope route
    pure (route, if reset then none else arm)

/-- Derive a prospective body's source base before any successful operation
or publication. No post-operation snapshot, frame, or guessed ready bit is an
input. Resource headers and public-call borrows are separate checks. -/
def prepareObservedActorBody?
    (image : RustDescriptorImage) (choreo : Choreo) (before : ActorProgressObservation)
    (previousPath nextPath scope event : Nat) : Option (DecodedRollRow × ActorProgressObservation) := do
    let data ← image.decodeActorOrder? choreo
    let previous ← data.normalized.paths[previousPath]?
    let next ← data.normalized.paths[nextPath]?
    let rolls ← image.decodeRollRows?
    let roll ← resolveUnique? (rolls.filter fun row => row.scope == scope)
    let source := canonicalProgramSource choreo
    let choices ← resetObservedChoices? source scope before.choices
    let base := resetObservedActor before roll choices
    if image.actorOrderCheck choreo && image.actorRollCheck choreo &&
        image.actorRouteCheck choreo && before.metadataMatches image data.rows &&
        before.valid &&
        roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength &&
        observedChoicesAgree source previous before &&
        observedChoicesAgree source next base &&
        (data.activeRows previous).all (fun row =>
          if roll.eventStart ≤ row.eventId && row.eventId < roll.eventStart + roll.eventLength
          then before.doneState.done row.eventId else true) &&
        observedSourcePrefixCheck data next before event then
      some (roll, base)
    else none

def ActorReentryCommitObservation.prepared?
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage)
    (choreo : Choreo) : Option (DecodedRollRow × ActorCommitObservation) :=
  if observedRollEntryHeadersMatch sample.before image sample.scope &&
      observedRollEntryBorrowMatches sample.before image sample.scope sample.event sample.view then do
    let (roll, base) ← prepareObservedActorBody? image choreo sample.before
      sample.previousPath sample.nextPath sample.scope sample.event
    let eff ← image.eventEffIndex? sample.event
    let published ← publishedObservedChoices? (canonicalProgramSource choreo) eff base.choices
    if decide (sample.after.choices = published) then
      some (roll, {
        pathIndex := sample.nextPath, event := sample.event, view := sample.view,
        before := base, after := sample.after })
    else none
  else none

def ActorReentryCommitObservation.check
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage)
    (choreo : Choreo) : Bool :=
  match sample.prepared? image choreo with
  | none => false
  | some (_, step) => step.check image choreo

/-- An owned first operation of a fresh visit checks the same derived base
against the actual accepted frame. It does not infer a visit from the envelope:
FIFO, replay exclusion and run isolation remain carrier obligations. -/
def ActorReentryCommitObservation.ownedCheck
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage)
    (choreo : Choreo) (wire : OwnedWireObservation) (session : Nat) : Bool :=
  match sample.prepared? image choreo with
  | none => false
  | some (_, step) => step.ownedCheck image choreo wire session

theorem accepted_owned_body_reentry_has_checked_owned_step
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat}
    (checked : sample.ownedCheck image choreo wire session = true) :
    ∃ roll step, sample.prepared? image choreo = some (roll, step) ∧
      step.ownedCheck image choreo wire session = true := by
  unfold ActorReentryCommitObservation.ownedCheck at checked
  cases prepared : sample.prepared? image choreo with
  | none => simp [prepared] at checked
  | some entry =>
      exact ⟨entry.1, entry.2, rfl, by simpa only [prepared] using checked⟩

theorem accepted_owned_body_reentry_also_checks_source_reset
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat}
    (checked : sample.ownedCheck image choreo wire session = true) :
    sample.check image choreo = true := by
  obtain ⟨roll, step, prepared, owned⟩ := accepted_owned_body_reentry_has_checked_owned_step checked
  simp only [ActorReentryCommitObservation.check, prepared]
  exact (Bool.and_eq_true_iff.mp owned).1

private theorem reset_observed_actor_done
    (snapshot : ActorProgressObservation) (roll : DecodedRollRow)
    (choices : List (Nat × Option RouteArm)) (event : Nat) :
    (resetObservedActor snapshot roll choices).doneState.done event =
      if roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength
      then false else snapshot.doneState.done event := by
  simp only [resetObservedActor, ActorProgressObservation.doneState,
    ActorProgressObservation.progress, List.getElem?_map, List.getElem?_zipIdx, Nat.zero_add]
  cases snapshot.steps[event]? <;> simp

/-- A checked fresh commit uses a base derived from its pre-operation cut.
The same derivation can therefore justify a prospective candidate without
assuming that the candidate has already committed. -/
theorem prepared_body_reentry_is_derived_before_commit
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    ∃ base, prepareObservedActorBody? image choreo sample.before
        sample.previousPath sample.nextPath sample.scope sample.event = some (roll, base) ∧
      step.before = base ∧ step.event = sample.event ∧ step.after = sample.after := by
  unfold ActorReentryCommitObservation.prepared? at prepared
  split at prepared
  next qualified =>
    obtain ⟨⟨actualRoll, base⟩, baseAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
    obtain ⟨eff, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
    obtain ⟨published, _, written⟩ := Option.bind_eq_some_iff.mp prepared
    split at written
    next accepted =>
      have same := Option.some.inj written
      cases same
      exact ⟨base, baseAt, rfl, rfl, rfl⟩
    next rejected => simp at written
  next rejected => simp at prepared

private theorem source_body_base_facts
    {image : RustDescriptorImage} {choreo : Choreo} {before base : ActorProgressObservation}
    {previousPath nextPath scope event : Nat} {roll : DecodedRollRow}
    (prepared : prepareObservedActorBody? image choreo before
      previousPath nextPath scope event = some (roll, base)) :
    image.actorRollCheck choreo = true ∧
      ∃ rows choices, image.decodeRollRows? = some rows ∧ roll ∈ rows ∧
        base = resetObservedActor before roll choices ∧
        roll.eventStart ≤ event ∧ event < roll.eventStart + roll.eventLength := by
  unfold prepareObservedActorBody? at prepared
  obtain ⟨data, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨previous, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨nextPath, _, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨rows, rowsAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨chosen, chosenAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨choices, _, allowed⟩ := Option.bind_eq_some_iff.mp prepared
  dsimp only at allowed
  split at allowed
  next guarded =>
    have same := Option.some.inj allowed
    cases same
    have member : roll ∈ rows := by
      have singleton := resolveUnique?_eq_some_iff.mp chosenAt
      have filtered : roll ∈ rows.filter (fun row => row.scope == scope) := by
        rw [singleton]
        simp
      exact (List.mem_filter.mp filtered).1
    simp only [Bool.and_eq_true, and_assoc] at guarded
    obtain ⟨_, ranges, _, _, _, start, stop, _, _, _, _⟩ := guarded
    exact ⟨ranges, rows, choices, rowsAt, member, rfl,
      of_decide_eq_true start, of_decide_eq_true stop⟩
  next rejected => simp at allowed

private theorem prepared_reentry_facts
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    image.actorRollCheck choreo = true ∧
      observedRollEntryHeadersMatch sample.before image sample.scope = true ∧
      observedRollEntryBorrowMatches sample.before image sample.scope sample.event sample.view = true ∧
      ∃ rows choices, image.decodeRollRows? = some rows ∧ roll ∈ rows ∧
        step.before = resetObservedActor sample.before roll choices ∧
        step.event = sample.event ∧ step.after = sample.after ∧
        roll.eventStart ≤ step.event ∧ step.event < roll.eventStart + roll.eventLength := by
  have qualified : (observedRollEntryHeadersMatch sample.before image sample.scope &&
      observedRollEntryBorrowMatches sample.before image sample.scope sample.event sample.view) = true := by
    unfold ActorReentryCommitObservation.prepared? at prepared
    cases flag : (observedRollEntryHeadersMatch sample.before image sample.scope &&
        observedRollEntryBorrowMatches sample.before image sample.scope sample.event sample.view) with
    | false => simp [flag] at prepared
    | true => rfl
  obtain ⟨base, baseAt, beforeAt, eventAt, afterAt⟩ :=
    prepared_body_reentry_is_derived_before_commit prepared
  obtain ⟨ranges, rows, choices, rowsAt, member, reset, start, stop⟩ := source_body_base_facts baseAt
  exact ⟨ranges, (Bool.and_eq_true_iff.mp qualified).1, (Bool.and_eq_true_iff.mp qualified).2,
    rows, choices, rowsAt, member, beforeAt.trans reset, eventAt, afterAt,
    eventAt.symm ▸ start, eventAt.symm ▸ stop⟩

/-- Reset changes progress and choices, not the retained Endpoint tags or
temporary borrow phases. This alone is not evidence of Rust value availability. -/
theorem prepared_body_reentry_preserves_value_identity
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    step.before.actor = sample.before.actor ∧ step.before.run = sample.before.run ∧
      step.before.views = sample.before.views := by
  obtain ⟨_, _, _, _, choices, _, _, base, _⟩ := prepared_reentry_facts prepared
  rw [base]
  exact ⟨rfl, rfl, rfl⟩

/-- The derived base belongs to this actual operation and its post-poll cut,
inside the exact reset body. It cannot substitute another event or snapshot. -/
theorem prepared_body_reentry_keeps_its_actual_operation
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation}
    (prepared : sample.prepared? image choreo = some (roll, step)) :
    step.event = sample.event ∧ step.after = sample.after ∧
      roll.eventStart ≤ step.event ∧ step.event < roll.eventStart + roll.eventLength := by
  obtain ⟨_, _, _, _, _, _, _, _, actual⟩ := prepared_reentry_facts prepared
  exact actual

theorem accepted_body_reentry_requires_entry_headers
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) :
    observedRollEntryHeadersMatch sample.before image sample.scope = true := by
  unfold ActorReentryCommitObservation.check at accepted
  cases found : sample.prepared? image choreo with
  | none => simp [found] at accepted
  | some entry => exact (prepared_reentry_facts found).2.1

theorem accepted_body_reentry_checks_entry_borrows
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) :
    observedRollEntryBorrowMatches sample.before image sample.scope sample.event sample.view = true := by
  unfold ActorReentryCommitObservation.check at accepted
  cases found : sample.prepared? image choreo with
  | none => simp [found] at accepted
  | some entry => exact (prepared_reentry_facts found).2.2.1

theorem accepted_body_reentry_borrowed_entry_is_its_payload
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {rows : OwnedEntryResources} {roll : DecodedRollRow} {entries : List (Nat × Nat)}
    {role reference : Nat} {view : ActorViewObservation} {sent : Option Nat}
    (accepted : sample.check image choreo = true)
    (rowsAt : image.decodeRollEntryResources? = some rows)
    (payloadAt : observedOutgoingPayload? image sample.event sample.view = some sent)
    (scopeAt : resolveUnique? (rows.filter fun row => row.1.scope == sample.scope) =
      some (roll, entries))
    (member : (role, reference) ∈ entries)
    (found : observedActorView? sample.before role = some view)
    (borrowed : view.operation = .sendPayload) : sent = some role := by
  have checked := accepted_body_reentry_checks_entry_borrows accepted
  simp only [observedRollEntryBorrowMatches, rowsAt, payloadAt, scopeAt] at checked
  exact observed_entry_borrow_requires_its_own_send checked member found borrowed

theorem accepted_body_reentry_uses_issued_entry_resource
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {columns : DecodedProgramColumns} {rows : OwnedEntryResources}
    {roll : DecodedRollRow} {entries : List (Nat × Nat)} {role reference : Nat}
    (accepted : sample.check image choreo = true)
    (columnsAt : image.decodeProgramColumns? = some columns)
    (rowsAt : image.decodeRollEntryResources? = some rows)
    (scopeAt : resolveUnique? (rows.filter fun row => row.1.scope == sample.scope) =
      some (roll, entries))
    (member : (role, reference) ∈ entries)
    (resolved : columns.selectedActor? sample.before.doneState.selected reference =
      some sample.before.actor) :
    ∃ view, observedActorView? sample.before role = some view ∧
      view.run = sample.before.run ∧ view.nonce % 2 = 1 := by
  have checked := accepted_body_reentry_requires_entry_headers accepted
  simp only [observedRollEntryHeadersMatch, columnsAt, rowsAt, scopeAt] at checked
  exact observed_entry_header_requires_issued_resource checked member resolved

def ActorReentryCommitObservation.SourceOrdered
    (sample : ActorReentryCommitObservation) (image : RustDescriptorImage) (choreo : Choreo) : Prop :=
    ∃ roll step rows,
      sample.prepared? image choreo = some (roll, step) ∧
      choreo.canonicalActorRollRows? image.role = some rows ∧ roll ∈ rows ∧
      (∀ event, step.before.doneState.done event =
        if roll.eventStart ≤ event && event < roll.eventStart + roll.eventLength
        then false else sample.before.doneState.done event) ∧
      step.SourceOrdered image choreo

/-- A checked atomic body reentry clears exactly a source-validated range,
preserves all outside progress, and commits a source-ordered step with the
matching local view publication. Choice reset/publication is checked by the
same prepared observation. This does not yet establish distributed liveness. -/
theorem accepted_body_reentry_restores_source_progress
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (accepted : sample.check image choreo = true) : sample.SourceOrdered image choreo := by
  unfold ActorReentryCommitObservation.SourceOrdered
  unfold ActorReentryCommitObservation.check at accepted
  cases found : sample.prepared? image choreo with
  | none => simp [found] at accepted
  | some entry =>
      rcases entry with ⟨roll, step⟩
      have checked : step.check image choreo = true := by simpa [found] using accepted
      obtain ⟨ranges, _, _, actual, choices, actualAt, member, base, _⟩ := prepared_reentry_facts found
      obtain ⟨_, _, rows, rowsAt, decoded⟩ := accepted_actor_rolls_bind_source_ranges ranges
      have same : actual = rows := Option.some.inj (actualAt.symm.trans decoded)
      refine ⟨roll, step, rows, rfl, rowsAt, by simpa only [← same] using member, ?_,
        accepted_actor_commit_matches_source_progress checked⟩
      intro event
      rw [base]
      exact reset_observed_actor_done sample.before roll choices event

/-- The first owned receive of a checked fresh visit publishes the exact
projected continuation, composes with ordered delivery, and retains at most one
usable authority. Sender publication is not a premise. The carrier supplies
run isolation and replay exclusion; the three-byte envelope has no visit tag. -/
theorem source_checked_reentry_receipt_preserves_unique_authority
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {roll : DecodedRollRow}
    {step : ActorCommitObservation} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (prepared : sample.prepared? image choreo = some (roll, step))
    (decoded : step.ownedFacts? image choreo = some facts)
    (receiving : facts.side = .receive)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered committed : carrier.State} {frame : TransportFrame}
    {before : TransportState}
    {state : EndpointTransfer.Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {rest : List TransportFrame} (initialView : Nat × Nat × Nat × Nat)
    (execution : EndpointTransfer.Execution (EndpointTransfer.initial initialView) state)
    (currentExact : abstraction current = .idle before) (wellFormed : before.WellFormed)
    (received : carrier.pollReceive current = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (identity : wire.identifies frame) (queued : state.inFlight = frame :: rest) :
    sample.SourceOrdered image choreo ∧
      step.before.actor = sample.before.actor ∧ step.before.run = sample.before.run ∧
      step.before.views = sample.before.views ∧
      step.after.actor = facts.binding.receiverActor ∧
      ∃ next after, EndpointTransfer.Step .receive state next ∧ next.usable.length ≤ 1 ∧
        next.usable = (step.after.actor, facts.afterView.header) :: state.usable ∧
        abstraction committed = .idle after ∧ before.delivered < after.delivered := by
  have owned : step.ownedCheck image choreo wire session = true := by
    simpa only [ActorReentryCommitObservation.ownedCheck, prepared] using checked
  obtain ⟨_, _, _, next, after, transition, unique, value, carrierAt, advances, _⟩ :=
    source_checked_carrier_receive_preserves_unique_authority owned decoded receiving profile
      initialView execution currentExact wellFormed received resolved identity queued
  obtain ⟨sameActor, sameRun, sameViews⟩ := prepared_body_reentry_preserves_value_identity prepared
  exact ⟨accepted_body_reentry_restores_source_progress
    (accepted_owned_body_reentry_also_checks_source_reset checked), sameActor, sameRun,
    sameViews, (checked_owned_receive_publishes_its_projected_actor owned decoded receiving),
    next, after, transition, unique, value, carrierAt, advances⟩

end Hibana
