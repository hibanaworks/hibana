import Hibana.ActorCommit
import Hibana.EndpointTransfer
import Hibana.Generation

namespace Hibana
open EndpointDelegation

/-- Fail closed if any source row cannot be interpreted. This is proof-only
translation validation, not a runtime progress or ownership table. -/
def allPresentTrue? {α : Type} (test : α → Option Bool) : List α → Option Bool
  | [] => some true
  | head :: tail =>
      match test head, allPresentTrue? test tail with
      | some first, some rest => some (first && rest)
      | _, _ => none

theorem all_present_true_checks_every_member
    {α : Type} {test : α → Option Bool} {items : List α}
    (checked : allPresentTrue? test items = some true) :
    ∀ item ∈ items, test item = some true := by
  induction items with
  | nil => simp
  | cons head tail ih =>
      cases headAt : test head with
      | none => simp [allPresentTrue?, headAt] at checked
      | some first =>
          cases tailAt : allPresentTrue? test tail with
          | none => simp [allPresentTrue?, headAt, tailAt] at checked
          | some rest =>
              have flags : first = true ∧ rest = true := by
                simpa [allPresentTrue?, headAt, tailAt] using checked
              intro item member
              rcases List.mem_cons.mp member with same | member
              · subst item; simpa [flags.1] using headAt
              · exact ih (by simpa [flags.2] using tailAt) item member

/-- A view is required by its logical send/receive or as the owned payload of
a send. A view with no own wire operation may still be required for a loan. -/
def sourceActionRequiresView (role : Nat) (binding : Binding) (side : Side) : Bool :=
  let key : EndpointKey := { component := 0, role }
  match side with
  | .receive => binding.message.receiver == key
  | .send | .local =>
      binding.message.sender == key || binding.message.payload == .endpoint key

def sourceRetirementRowComplete?
    (snapshot : ActorProgressObservation) (role : Nat)
    (path : NormalizedPath) (row : DecodedActorRow) : Option Bool := do
  let (_, binding) ← path.bindings.find? fun entry => entry.1 == row.actionId.event
  let step ← snapshot.steps[row.eventId]?
  pure (step.done || !sourceActionRequiresView role binding row.actionId.side)

theorem source_complete_row_has_no_pending_view_requirement
    {snapshot : ActorProgressObservation} {role : Nat}
    {path : NormalizedPath} {row : DecodedActorRow}
    (checked : sourceRetirementRowComplete? snapshot role path row = some true) :
    ∃ entry step,
      path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
      snapshot.steps[row.eventId]? = some step ∧
      (sourceActionRequiresView role entry.2 row.actionId.side = true → step.done = true) := by
  cases entryAt : path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
  | none => simp [sourceRetirementRowComplete?, entryAt] at checked
  | some entry =>
      cases stepAt : snapshot.steps[row.eventId]? with
      | none => simp [sourceRetirementRowComplete?, entryAt, stepAt] at checked
      | some step =>
          refine ⟨entry, step, rfl, rfl, ?_⟩
          intro required
          simpa [sourceRetirementRowComplete?, entryAt, stepAt, required] using checked

/-- Completion in the current selected visit. All source paths compatible
with the committed choices are checked; an unknown choice is not an empty
branch. This does not declare future rolled visits complete. -/
def sourceViewComplete?
    (snapshot : ActorProgressObservation) (image : RustDescriptorImage)
    (choreo : Choreo) (role : Nat) : Option Bool :=
  match image.decodeActorOrder? choreo with
  | none => none
  | some data =>
      if image.actorOrderCheck choreo && snapshot.valid && snapshot.metadataMatches image data.rows then
        let paths := data.normalized.paths.filter fun path =>
          observedChoicesAgree (canonicalProgramSource choreo) path snapshot
        if paths.isEmpty then none else
          allPresentTrue? (fun path =>
            allPresentTrue? (sourceRetirementRowComplete? snapshot role path) (data.activeRows path)) paths
      else none

theorem source_completion_checks_every_compatible_path
    {snapshot : ActorProgressObservation} {image : RustDescriptorImage}
    {choreo : Choreo} {role : Nat}
    (complete : sourceViewComplete? snapshot image choreo role = some true) :
    ∃ data, image.decodeActorOrder? choreo = some data ∧
      ∀ path ∈ data.normalized.paths,
        observedChoicesAgree (canonicalProgramSource choreo) path snapshot = true →
        ∀ row ∈ data.activeRows path,
          sourceRetirementRowComplete? snapshot role path row = some true := by
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [sourceViewComplete?, dataAt] at complete
  | some data =>
      by_cases good :
          (image.actorOrderCheck choreo && snapshot.valid && snapshot.metadataMatches image data.rows) = true
      · let paths := data.normalized.paths.filter fun path =>
          observedChoicesAgree (canonicalProgramSource choreo) path snapshot
        have result :
            (if paths.isEmpty then none else
              allPresentTrue? (fun path =>
                allPresentTrue? (sourceRetirementRowComplete? snapshot role path) (data.activeRows path)) paths) = some true := by
          simpa only [sourceViewComplete?, dataAt, good, ite_true] using complete
        cases empty : paths.isEmpty with
        | true => simp only [empty, ite_true, reduceCtorEq] at result
        | false =>
          have checked : allPresentTrue? (fun path =>
              allPresentTrue? (sourceRetirementRowComplete? snapshot role path) (data.activeRows path)) paths = some true := by
            simpa only [empty, Bool.false_eq_true, ite_false] using result
          refine ⟨data, rfl, ?_⟩
          intro path member agrees row rowMember
          have inPaths : path ∈ paths := List.mem_filter.mpr ⟨member, agrees⟩
          have pathChecked := all_present_true_checks_every_member checked path inPaths
          exact all_present_true_checks_every_member pathChecked row rowMember
      · simp [sourceViewComplete?, dataAt, good] at complete

theorem source_completed_view_has_no_pending_action
    {snapshot : ActorProgressObservation} {image : RustDescriptorImage}
    {choreo : Choreo} {role : Nat}
    (complete : sourceViewComplete? snapshot image choreo role = some true) :
    ∃ data, image.decodeActorOrder? choreo = some data ∧
      ∀ path ∈ data.normalized.paths,
        observedChoicesAgree (canonicalProgramSource choreo) path snapshot = true →
        ∀ row ∈ data.activeRows path, ∃ entry step,
          path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
          snapshot.steps[row.eventId]? = some step ∧
          (sourceActionRequiresView role entry.2 row.actionId.side = true → step.done = true) := by
  obtain ⟨data, decoded, paths⟩ := source_completion_checks_every_compatible_path complete
  refine ⟨data, decoded, ?_⟩
  intro path member agrees row rowMember
  exact source_complete_row_has_no_pending_view_requirement (paths path member agrees row rowMember)

def retirementFirstFault
    (before : Option SessionFault) (operation : PublicOperationPhase) (complete : Bool) :
    Option SessionFault :=
  if operation = .idle ∧ complete = true then before
  else before.orElse (fun _ => some .endpointDropped)

def observedGeneration (fault : Option SessionFault) : SessionGenerationState :=
  match fault with
  | none => .live
  | some cause => .poisoned cause

def retirementGenerationAction
    (operation : PublicOperationPhase) (complete : Bool) : SessionGenerationAction :=
  if operation = .idle ∧ complete = true then .releaseRemaining
  else .poison .endpointDropped

theorem retirement_fault_matches_existing_generation_model
    (before : Option SessionFault) (operation : PublicOperationPhase) (complete : Bool) :
    applySessionGenerationAction (observedGeneration before)
      (retirementGenerationAction operation complete) =
        some (observedGeneration (retirementFirstFault before operation complete)) := by
  cases before <;>
    by_cases finished : operation = .idle ∧ complete = true <;>
    simp [retirementGenerationAction, retirementFirstFault, observedGeneration,
      applySessionGenerationAction, finished]

structure ViewRetirementObservation where
  view : Nat
  before : ActorProgressObservation
  after : ActorProgressObservation
  beforeFault : Option SessionFault
  afterFault : Option SessionFault

def ViewRetirementObservation.progressMatches (sample : ViewRetirementObservation) : Bool :=
  sample.after.actor == sample.before.actor && sample.after.run == sample.before.run &&
    sample.after.counter == sample.before.counter &&
    decide (sample.after.steps = sample.before.steps) &&
    decide (sample.after.choices = sample.before.choices)

def ViewRetirementObservation.headersMatch
    (sample : ViewRetirementObservation) (current : ActorViewObservation) : Bool :=
  sample.before.viewIssued sample.view && sample.after.valid &&
    sample.after.liveViews + 1 == sample.before.liveViews &&
    decide (sample.after.viewHeaders =
      (replaceObservedActorView sample.before sample.view 0 (current.nonce + 1)).map
        ActorViewObservation.header)

def ViewRetirementObservation.check
    (sample : ViewRetirementObservation) (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  match sourceViewComplete? sample.before image choreo sample.view,
      observedActorView? sample.before sample.view with
  | some complete, some current =>
      (sample.progressMatches && sample.headersMatch current) &&
        decide (sample.afterFault = retirementFirstFault sample.beforeFault current.operation complete)
  | _, _ => false

theorem accepted_retirement_has_checked_source_and_headers
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    ∃ complete current,
      sourceViewComplete? sample.before image choreo sample.view = some complete ∧
      observedActorView? sample.before sample.view = some current ∧
      sample.progressMatches = true ∧ sample.headersMatch current = true ∧
      sample.afterFault = retirementFirstFault sample.beforeFault current.operation complete := by
  cases sourceAt : sourceViewComplete? sample.before image choreo sample.view with
  | none => simp [ViewRetirementObservation.check, sourceAt] at checked
  | some complete =>
      cases currentAt : observedActorView? sample.before sample.view with
      | none => simp [ViewRetirementObservation.check, sourceAt, currentAt] at checked
      | some current =>
          have fields :
              (sample.progressMatches = true ∧ sample.headersMatch current = true) ∧
                sample.afterFault = retirementFirstFault sample.beforeFault current.operation complete := by
            simpa [ViewRetirementObservation.check, sourceAt, currentAt] using checked
          exact ⟨complete, current, rfl, rfl, fields.1.1, fields.1.2, fields.2⟩

theorem accepted_retirement_preserves_first_fault
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {cause : SessionFault} (checked : sample.check image choreo = true)
    (existing : sample.beforeFault = some cause) : sample.afterFault = some cause := by
  obtain ⟨complete, current, _, _, _, _, fault⟩ :=
    accepted_retirement_has_checked_source_and_headers checked
  rw [fault, existing]
  by_cases finished : current.operation = .idle ∧ complete = true <;>
    simp [retirementFirstFault, finished]

theorem retirement_with_pending_source_disables_publication
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true)
    (pending : sourceViewComplete? sample.before image choreo sample.view = some false) :
    (observedGeneration sample.afterFault).publicationPermitted = false := by
  obtain ⟨complete, current, sourceAt, _, _, _, fault⟩ :=
    accepted_retirement_has_checked_source_and_headers checked
  have same : complete = false := Option.some.inj (sourceAt.symm.trans pending)
  rw [fault, same]
  cases sample.beforeFault <;>
    simp [retirementFirstFault, observedGeneration, SessionGenerationState.publicationPermitted]

theorem live_retirement_requires_source_completion
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true)
    (live : sample.afterFault = none) :
    sourceViewComplete? sample.before image choreo sample.view = some true ∧
      ∃ current, observedActorView? sample.before sample.view = some current ∧
        current.operation = .idle := by
  obtain ⟨complete, current, sourceAt, currentAt, _, _, fault⟩ :=
    accepted_retirement_has_checked_source_and_headers checked
  have finished : current.operation = .idle ∧ complete = true := by
    by_cases finished : current.operation = .idle ∧ complete = true
    · exact finished
    · cases sample.beforeFault <;> simp [retirementFirstFault, finished, live] at fault
  exact ⟨finished.2 ▸ sourceAt, current, currentAt, finished.1⟩

theorem live_retirement_removes_no_pending_source_action
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) (live : sample.afterFault = none) :
    ∃ data, image.decodeActorOrder? choreo = some data ∧
      ∀ path ∈ data.normalized.paths,
        observedChoicesAgree (canonicalProgramSource choreo) path sample.before = true →
        ∀ row ∈ data.activeRows path, ∃ entry step,
          path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
          sample.before.steps[row.eventId]? = some step ∧
          (sourceActionRequiresView sample.view entry.2 row.actionId.side = true → step.done = true) :=
  source_completed_view_has_no_pending_action (live_retirement_requires_source_completion checked live).1

theorem retirement_realizes_existing_generation_action
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    ∃ complete current,
      sourceViewComplete? sample.before image choreo sample.view = some complete ∧
      observedActorView? sample.before sample.view = some current ∧
      applySessionGenerationAction (observedGeneration sample.beforeFault)
        (retirementGenerationAction current.operation complete) = some (observedGeneration sample.afterFault) := by
  obtain ⟨complete, current, sourceAt, currentAt, _, _, fault⟩ :=
    accepted_retirement_has_checked_source_and_headers checked
  exact ⟨complete, current, sourceAt, currentAt, fault ▸
    retirement_fault_matches_existing_generation_model sample.beforeFault current.operation complete⟩

/-- Ordinary transfer boundaries conserve authority exactly. Only explicit
abandonment, application-value drop or frame fencing can reduce it. -/
theorem transfer_step_preserves_authority_without_retirement
    {View Frame : Type} {action : EndpointTransfer.Action}
    {before after : EndpointTransfer.Observation View Frame}
    (step : EndpointTransfer.Step action before after)
    (preparing : action ≠ .abandonPreparation) (value : action ≠ .dropValue)
    (frame : action ≠ .fenceFrame) : after.authorityCount = before.authorityCount := by
  cases step <;>
    simp_all [EndpointTransfer.Observation.authorityCount, List.length_append,
      List.length_cons, List.length_nil] <;> omega

theorem lost_transfer_authority_requires_explicit_retirement
    {View Frame : Type} {action : EndpointTransfer.Action}
    {before after : EndpointTransfer.Observation View Frame}
    (step : EndpointTransfer.Step action before after)
    (lost : after.authorityCount < before.authorityCount) :
    action = .abandonPreparation ∨ action = .dropValue ∨ action = .fenceFrame := by
  cases step <;>
    simp_all [EndpointTransfer.Observation.authorityCount, List.length_append,
      List.length_cons, List.length_nil] <;> omega

/-- An explicit retirement consumes exactly one authority. Private witnesses
left in completed send futures do not restore that authority. -/
theorem retirement_step_consumes_one_authority
    {View Frame : Type} {action : EndpointTransfer.Action}
    {before after : EndpointTransfer.Observation View Frame}
    (step : EndpointTransfer.Step action before after)
    (retired : action = .abandonPreparation ∨ action = .dropValue ∨ action = .fenceFrame) :
    after.authorityCount + 1 = before.authorityCount := by
  cases step <;>
    simp_all [EndpointTransfer.Observation.authorityCount, List.length_append,
      List.length_cons] <;> omega

/-- Loss in an arbitrary finite execution has a concrete retirement boundary
with a prefix and suffix. It cannot be explained by an ordinary send, receive,
or the destruction of a completed sender's private witness. -/
theorem transfer_execution_loss_has_retirement_boundary
    {View Frame : Type} {before after : EndpointTransfer.Observation View Frame}
    (execution : EndpointTransfer.Execution before after)
    (lost : after.authorityCount < before.authorityCount) :
    ∃ middle following action,
      EndpointTransfer.Execution before middle ∧
      EndpointTransfer.Step action middle following ∧
      (action = .abandonPreparation ∨ action = .dropValue ∨ action = .fenceFrame) ∧
      EndpointTransfer.Execution following after := by
  induction execution with
  | refl => exact False.elim (Nat.lt_irrefl _ lost)
  | @tail middle action last history step ih =>
      by_cases earlier : middle.authorityCount < before.authorityCount
      · obtain ⟨retiring, retired, operation, prior, boundary, kind, suffix⟩ := ih earlier
        exact ⟨retiring, retired, operation, prior, boundary, kind, .tail suffix step⟩
      · have bound := EndpointTransfer.transfer_execution_cannot_duplicate_authority history
        have decreased : last.authorityCount < middle.authorityCount := by omega
        exact ⟨middle, last, action, history, step,
          lost_transfer_authority_requires_explicit_retirement step decreased, .refl _⟩

/-- Proof-only location of the one authority already represented by Rust
values/futures and carrier receipts. This is not an owner/progress table.
Completed sender witnesses may coexist with any of the three locations. -/
def TransferAuthorityLocated (state : EndpointTransfer.Observation View Frame) : Prop :=
  (∃ view, state.usable = [view] ∧ state.preparing = [] ∧ state.inFlight = []) ∨
  (∃ view, state.usable = [] ∧ state.preparing = [view] ∧ state.inFlight = []) ∨
  (∃ frame, state.usable = [] ∧ state.preparing = [] ∧ state.inFlight = [frame])

theorem located_transfer_authority_has_count_one
    {View Frame : Type} {state : EndpointTransfer.Observation View Frame}
    (located : TransferAuthorityLocated state) : state.authorityCount = 1 := by
  rcases located with ⟨view, usable, preparing, frames⟩ |
    ⟨view, usable, preparing, frames⟩ | ⟨frame, usable, preparing, frames⟩ <;>
    simp [EndpointTransfer.Observation.authorityCount, usable, preparing, frames]

theorem count_one_transfer_authority_has_one_location
    {View Frame : Type} {state : EndpointTransfer.Observation View Frame}
    (count : state.authorityCount = 1) : TransferAuthorityLocated state := by
  cases usable : state.usable with
  | cons view rest =>
      have lengths : rest.length = 0 ∧ state.preparing.length = 0 ∧ state.inFlight.length = 0 := by
        simp only [EndpointTransfer.Observation.authorityCount, usable, List.length_cons] at count
        exact ⟨by omega, by omega, by omega⟩
      exact .inl ⟨view, by simpa [List.length_eq_zero_iff.mp lengths.1] using usable,
        List.length_eq_zero_iff.mp lengths.2.1, List.length_eq_zero_iff.mp lengths.2.2⟩
  | nil =>
      cases preparing : state.preparing with
      | cons view rest =>
          have lengths : rest.length = 0 ∧ state.inFlight.length = 0 := by
            simp only [EndpointTransfer.Observation.authorityCount, usable, preparing,
              List.length_nil, List.length_cons] at count
            exact ⟨by omega, by omega⟩
          exact .inr (.inl ⟨view, usable,
            by simpa [List.length_eq_zero_iff.mp lengths.1] using preparing,
            List.length_eq_zero_iff.mp lengths.2⟩)
      | nil =>
          cases frames : state.inFlight with
          | nil => simp [EndpointTransfer.Observation.authorityCount, usable, preparing, frames] at count
          | cons frame rest =>
              have length : rest.length = 0 := by
                simp only [EndpointTransfer.Observation.authorityCount, usable, preparing,
                  frames, List.length_nil, List.length_cons] at count
                omega
              exact .inr (.inr ⟨frame, usable, preparing,
                by simpa [List.length_eq_zero_iff.mp length] using frames⟩)

/-- For every execution from the unique initial issue, the required resource
is located or there is an explicit retirement boundary in its history. This
does not itself establish source/future/frame correspondence or fresh visits;
those observations must connect a boundary to completion or observable fault. -/
theorem initial_transfer_execution_is_located_or_has_retired
    {View Frame : Type} {initialView : View}
    {state : EndpointTransfer.Observation View Frame}
    (execution : EndpointTransfer.Execution (EndpointTransfer.initial initialView) state) :
    TransferAuthorityLocated state ∨
      ∃ middle following action,
        EndpointTransfer.Execution (EndpointTransfer.initial initialView) middle ∧
        EndpointTransfer.Step action middle following ∧
        (action = .abandonPreparation ∨ action = .dropValue ∨ action = .fenceFrame) ∧
        EndpointTransfer.Execution following state := by
  by_cases one : state.authorityCount = 1
  · exact .inl (count_one_transfer_authority_has_one_location one)
  · have bound := EndpointTransfer.one_binding_has_at_most_one_transferable_authority execution
    have empty : state.authorityCount < 1 := by omega
    have lost : state.authorityCount <
        (EndpointTransfer.initial initialView : EndpointTransfer.Observation View Frame).authorityCount := by
      simpa [EndpointTransfer.initial, EndpointTransfer.Observation.authorityCount] using empty
    exact .inr (transfer_execution_loss_has_retirement_boundary execution lost)

end Hibana
